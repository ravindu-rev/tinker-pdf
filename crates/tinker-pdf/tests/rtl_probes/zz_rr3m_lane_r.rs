//! `zz_rr3m_laneR` (laneR, lane R (round 5)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_laneR.rs`, which is the probe as it ran.
//!
//! Lane R probe: K-11 finding V1.

use super::common::{rep, run_book, Case, Shape, AR, HE, HEP};

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
    // Lane R (K-11 finding V1). Families:
    //  price/  right-to-left lines that open with a mark and then `(W) W`
    //          after a forced break: the line the extension reads as a
    //          dir-less `(W) W<mark>` (the ellipsis's price, and every other
    //          new ending's), with controls that must not move;
    //  soft/   the same marks after a space in running right-to-left text,
    //          where a soft wrap may open a line with them (UAX #14 LB18
    //          breaks after a space before LB19 or LB22 is asked);
    //  end/    dir-less `(W) W<end>` lines, the fix's own shape, for every
    //          new ending and controls;
    //  t/      the epub_conservation test's candidate books.
    let mut out: Vec<Case> = Vec::new();
    let price_marks: [(&str, &str); 21] = [
        ("ell", "\u{2026}"),
        ("dots3", "..."),
        ("dots2", ".."),
        ("one-dot", "\u{2024}"),
        ("two-dot", "\u{2025}"),
        ("vert", "\u{FE19}"),
        ("qe", "?!"),
        ("ee", "!!"),
        ("eq", "!?"),
        ("qq", "??"),
        ("quot-stop", "\"."),
        ("rdq-stop", "\u{201D}."),
        ("raq-stop", "\u{BB}."),
        ("apos-stop", "'."),
        ("rdq-ell", "\u{201D}\u{2026}"),
        ("quot-dots", "\"..."),
        ("stop", "."),
        ("q", "?"),
        ("acomma", "\u{60C}"),
        ("rdq", "\u{201D}"),
        ("quot", "\""),
    ];
    let brackets = [("(", ")"), ("[", "]"), ("{", "}")];
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE), ("hep", "he", HEP)] {
        let s = |i: usize| w[i % w.len()];
        for (mi, (mn, m)) in price_marks.iter().enumerate() {
            for (bi, (o, c)) in brackets.iter().enumerate() {
                let br = format!(
                    "{} {}<br/>{m}{o}{}{c} {} {}",
                    s(mi),
                    s(mi + 1),
                    s(mi + 2),
                    s(mi + 3),
                    s(mi + 4)
                );
                let brb = format!(
                    "{} {}<br/>{m}<b>{o}</b>{}<b>{c}</b> {} {}",
                    s(mi),
                    s(mi + 1),
                    s(mi + 2),
                    s(mi + 3),
                    s(mi + 4)
                );
                out.push((
                    format!("price/{sc}/{mn}/b{bi}/br/rtl"),
                    lang,
                    format!("<p dir=\"rtl\">{br}</p>"),
                ));
                out.push((
                    format!("price/{sc}/{mn}/b{bi}/brb/rtl"),
                    lang,
                    format!("<p dir=\"rtl\">{brb}</p>"),
                ));
                if bi == 0 {
                    let soft = rep(2, |i| {
                        format!(
                            "{} {} {} {m}{o}{}{c} {} {}.",
                            s(i + mi),
                            s(i + mi + 1),
                            s(i + mi + 2),
                            s(i + mi + 3),
                            s(i + mi + 4),
                            s(i + mi + 5)
                        )
                    });
                    out.push((
                        format!("soft/{sc}/{mn}/b{bi}/rtl"),
                        lang,
                        format!("<p dir=\"rtl\">{soft}</p>"),
                    ));
                }
            }
        }
    }
    let ends: [(&str, &str); 20] = [
        ("dots3", "..."),
        ("qe", "?!"),
        ("ee", "!!"),
        ("eq", "!?"),
        ("stop-quot", ".\""),
        ("stop-raq", ".\u{BB}"),
        ("stop-rdq", ".\u{201D}"),
        ("ell", "\u{2026}"),
        ("stop-apos", ".'"),
        ("q-rdq", "?\u{201D}"),
        ("e-raq", "!\u{BB}"),
        ("dots3-rdq", "...\u{201D}"),
        ("one-dot", "\u{2024}"),
        ("two-dot", "\u{2025}"),
        ("vert", "\u{FE19}"),
        ("dots2", ".."),
        ("qq", "??"),
        ("stop-paren", ".)"),
        ("rdq", "\u{201D}"),
        ("stop", "."),
    ];
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE), ("hep", "he", HEP)] {
        let s = |i: usize| w[i % w.len()];
        for (ei, (en, e)) in ends.iter().enumerate() {
            let shapes: Vec<Shape<'_>> = vec![
                (
                    "b-paren",
                    Box::new(move |i| {
                        format!(
                            "{} {} {} <b>(</b>{}<b>)</b> {}{e}",
                            s(i + ei),
                            s(i + ei + 1),
                            s(i + ei + 2),
                            s(i + ei + 3),
                            s(i + ei + 4)
                        )
                    }),
                ),
                (
                    "paren",
                    Box::new(move |i| {
                        format!(
                            "{} {} {} ({}) {}{e}",
                            s(i + ei),
                            s(i + ei + 1),
                            s(i + ei + 2),
                            s(i + ei + 3),
                            s(i + ei + 4)
                        )
                    }),
                ),
                (
                    "square",
                    Box::new(move |i| {
                        format!(
                            "{} {} {} [{}] {}{e}",
                            s(i + ei),
                            s(i + ei + 1),
                            s(i + ei + 2),
                            s(i + ei + 3),
                            s(i + ei + 4)
                        )
                    }),
                ),
            ];
            for (name, shape) in &shapes {
                out.push((
                    format!("end/{sc}/{en}/{name}/one"),
                    lang,
                    format!("<p>{}</p>", shape(0)),
                ));
                out.push((
                    format!("end/{sc}/{en}/{name}/three"),
                    lang,
                    format!("<p>{}</p>", rep(3, shape)),
                ));
            }
        }
    }
    for e in ["?!", ".\"", "\u{2026}", "..."] {
        out.push((
            format!("t/ar/{e}"),
            "ar",
            format!("<p>جميل بيت كبير <b>(</b>نور<b>)</b> يوم{e}</p>"),
        ));
        out.push((
            format!("t/ar2/{e}"),
            "ar",
            format!("<p>كتاب مدرسة الطالب <b>(</b>جميل<b>)</b> بيت{e}</p>"),
        ));
        out.push((
            format!("t/he/{e}"),
            "he",
            format!("<p>שלום עולם ספר <b>(</b>בית<b>)</b> יום{e}</p>"),
        ));
        out.push((
            format!("t/he7/{e}"),
            "he",
            format!("<p>שלום עולם ספר בית יום אור יום <b>(</b>בית<b>)</b> יום{e}</p>"),
        ));
    }
    out
}

fn styles() -> Vec<String> {
    let mut out = Vec::new();
    for m in ["30%", "34%", "38%", "40%", "42%", "44%", "46%"] {
        out.push(format!("p {{ margin: 0 {m} }}"));
    }
    out.push("p { text-align: justify; margin: 0 38% }".to_owned());
    out.push("body { font-family: sans-serif } p { margin: 0 40% }".to_owned());
    for ws in ["0.5em", "0.6em", "1em"] {
        out.push(format!("p {{ word-spacing: {ws}; margin: 0 20% }}"));
    }
    for ls in ["0.25em", "0.5em"] {
        out.push(format!(
            "p {{ letter-spacing: {ls}; text-align: justify; margin: 0 25% }}"
        ));
    }
    out.push("p { letter-spacing: 0.2em; word-spacing: 0.4em; margin: 0 30% }".to_owned());
    out.push("body { font-family: monospace } p { word-spacing: 0.5em; text-align: justify; margin: 0 25% }".to_owned());
    out.push("body { font-family: sans-serif } p { margin: 0 34% }".to_owned());
    out.push("body { font-family: sans-serif } p { margin: 0 42% }".to_owned());
    out.push("body { font-family: monospace } p { margin: 0 38% }".to_owned());
    out
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
