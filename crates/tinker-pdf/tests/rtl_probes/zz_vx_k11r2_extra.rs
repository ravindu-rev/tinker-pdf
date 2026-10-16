//! `zz_vx_k11r2_extra` (vx, round-5 review (confirm)): the books of
//! `tools/probes/rtl/probes/pdf/zz_vx_k11r2_extra.rs`, which is the probe as it ran.
//!
//! Probe: K-11 round 2. Shapes the round-1 review
//! named and their neighbours — left-to-right paragraphs with list markers,
//! smileys and glosses wrapped across lines; right-to-left quotation marks,
//! nested brackets and wrapped bracketed runs; letter-spaced right-to-left
//! text (`Tc`); Persian and Urdu letters outside the Arabic block. One ROW per
//! book: style, name, content, logical, first divergence, short text.

use super::common::strip;
use super::vov::styled_book;
use crate::epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::Document;

fn esc(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == '\n' {
                "|".to_owned()
            } else {
                c.to_string()
            }
        })
        .collect()
}

const AR: [&str; 8] = [
    "عوامل",
    "عدّة",
    "وقد",
    "أصبحنا",
    "نعرف",
    "الآن",
    "الموادّ",
    "تكبح",
];

const HE: [&str; 8] = ["שָׁלוֹם", "עוֹלָם", "מִלְחָמָה", "סֵפֶר", "בַּיִת", "גָּדוֹל", "אוֹר", "יוֹם"];

// Persian and Urdu: پ چ ژ گ (Arabic block), ک ی (Arabic block), ے ٹ ڈ ڑ ں ھ
// (Arabic block), and Arabic Supplement letters ݐ ݑ ݒ (U+0750..) in a made-up
// word, so that a justified line of them is right to left by Bidi_Class.
const FA: [&str; 6] = ["پژوهش", "گفتگو", "کتابخانه", "زندگی", "چگونه", "ݐݑݒݓ"];

fn books() -> Vec<(String, &'static str, String)> {
    let mut out = Vec::new();
    for (script, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        // Left-to-right paragraphs whose brackets do not pair line by line.
        out.push((format!("{script}/en/list-markers"), "en", format!(
            "<p>Step 1) open the file ({}) and 2) close it, then 3) read ({} {}) once more and 4) stop here for now.</p>",
            s(0), s(1), s(2))));
        out.push((format!("{script}/en/letter-list"), "en", format!(
            "<p>a) {} {} b) {} (see {}) c) the end of the list and d) more words after it to fill a line.</p>",
            s(0), s(1), s(2), s(3))));
        out.push((format!("{script}/en/smiley"), "en", format!(
            "<p>Great :) {} (yes) {} and then some more English words :) to make the line wrap here.</p>",
            s(0), s(1))));
        out.push((format!("{script}/en/two-glosses"), "en", format!(
            "<p>Peace ({} {}) and war ({} {} {}) are the words in the text, and the gloss goes on.</p>",
            s(0), s(1), s(2), s(3), s(4))));
        out.push((format!("{script}/en/gloss-wraps"), "en", format!(
            "<p>The long gloss ({} {} {} {} {} {}) wraps across the end of a line in this English paragraph.</p>",
            s(0), s(1), s(2), s(3), s(4), s(5))));
        // Right-to-left paragraphs: quotation marks, nesting, wrapping.
        out.push((
            format!("{script}/rtl/curly-ends"),
            lang,
            format!(
                "<p dir=\"rtl\">{} {} {} {} “histone deacetylase”</p>",
                s(0),
                s(1),
                s(2),
                s(3)
            ),
        ));
        out.push((
            format!("{script}/rtl/guillemets-ends"),
            lang,
            format!(
                "<p dir=\"rtl\">{} {} {} {} «histone deacetylase»</p>",
                s(0),
                s(1),
                s(2),
                s(3)
            ),
        ));
        out.push((
            format!("{script}/rtl/curly-mid"),
            lang,
            format!(
                "<p dir=\"rtl\">{} {} “histone deacetylase” {} {}.</p>",
                s(0),
                s(1),
                s(2),
                s(3)
            ),
        ));
        out.push((
            format!("{script}/rtl/nested"),
            lang,
            format!(
                "<p dir=\"rtl\">{} (see [HDAC] here). {} {}. {} (see [SIRT] there). {} {}.</p>",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4),
                s(5)
            ),
        ));
        out.push((format!("{script}/rtl/wrapped-latin"), lang, format!(
            "<p dir=\"rtl\">{} {} (the histone deacetylase family of enzymes in the cell nucleus). {} {} {}.</p>",
            s(0), s(1), s(2), s(3), s(4))));
        out.push((
            format!("{script}/rtl/two-latin"),
            lang,
            format!(
                "<p dir=\"rtl\">{} (histone deacetylase) {} (sirtuin family) {} {}.</p>",
                s(0),
                s(1),
                s(2),
                s(3)
            ),
        ));
        out.push((
            format!("{script}/rtl/list"),
            lang,
            format!(
                "<p dir=\"rtl\">1) {} {} 2) {} (HDAC) {} 3) {}.</p>",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
        ));
        out.push((
            format!("{script}/rtl/latin-ends"),
            lang,
            format!(
                "<p dir=\"rtl\">HDAC (histone deacetylase) {} {} (enzyme)</p>",
                s(0),
                s(1)
            ),
        ));
        out.push((
            format!("{script}/rtl/acr-full-stop"),
            lang,
            format!(
                "<p dir=\"rtl\">{} {} (HDAC). {} {} {}. {} (2010). {} {}.</p>",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4),
                s(5),
                s(6),
                s(7)
            ),
        ));
        out.push((
            format!("{script}/none/acr-full-stop"),
            lang,
            format!(
                "<p>{} {} (HDAC). {} {} {}. {} (2010). {} {}.</p>",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4),
                s(5),
                s(6),
                s(7)
            ),
        ));
        // Neutral runs of their own (the writer's level order).
        out.push((
            format!("{script}/rtl/italic-opens"),
            lang,
            format!("<p dir=\"rtl\"><i>?!</i> {}</p>", s(0)),
        ));
        out.push((
            format!("{script}/rtl/italic-opens-two"),
            lang,
            format!("<p dir=\"rtl\"><i>...</i> {} {}</p>", s(0), s(1)),
        ));
        out.push((
            format!("{script}/rtl/italic-mid"),
            lang,
            format!("<p dir=\"rtl\">{} <i>?!</i> {}</p>", s(0), s(1)),
        ));
        out.push((
            format!("{script}/en/rle"),
            "en",
            "<p>x &#x202B;?!&#x202C; y</p>".to_owned(),
        ));
        out.push((
            format!("{script}/en/rlo"),
            "en",
            "<p>x &#x202E;?!&#x202C; y</p>".to_owned(),
        ));
        out.push((
            format!("{script}/en/rli"),
            "en",
            "<p>x &#x2067;?!&#x2069; y</p>".to_owned(),
        ));
        out.push((
            format!("{script}/en/rle-word"),
            lang,
            format!("<p>x &#x202B;{} ?!&#x202C; y</p>", s(0)),
        ));
    }
    // Persian and Urdu, justified: a right-to-left word space carries the pen.
    let fa = |i: usize| FA[i % FA.len()];
    let mut long = String::new();
    for i in 0..18 {
        long.push_str(fa(i));
        long.push(' ');
    }
    out.push((
        "fa/rtl/justified".to_owned(),
        "fa",
        format!("<p dir=\"rtl\">{long}.</p>"),
    ));
    out.push((
        "fa/rtl/bracket".to_owned(),
        "fa",
        format!(
            "<p dir=\"rtl\">{} {} (HDAC). {} {} {} {}.</p>",
            fa(0),
            fa(1),
            fa(2),
            fa(3),
            fa(5),
            fa(4)
        ),
    ));
    out.push((
        "fa/none/justified".to_owned(),
        "fa",
        format!("<p>{long}.</p>"),
    ));
    out
}

fn probe() {
    let styles = [
        "",
        "p { text-align: justify }",
        "p { text-align: justify; margin: 0 20% }",
        "p { text-align: justify; margin: 0 35% }",
        "p { margin: 0 25% }",
        "p { margin: 0 40% }",
        "body { font-family: sans-serif } p { text-align: justify; margin: 0 15% }",
        "p { letter-spacing: 3px }",
        "p { letter-spacing: 0.3em; margin: 0 20% }",
        "p { font-family: monospace; letter-spacing: 8px }",
        "p { text-align: justify; letter-spacing: 2px; margin: 0 30% }",
    ];
    let (mut rows, mut fails) = (0, 0);
    for (si, style) in styles.iter().enumerate() {
        for (name, lang, body) in books() {
            for (label, body) in [("pointed", body.clone()), ("twin", strip(&body))] {
                let bytes = styled_book(lang, style, &body);
                let doc = match Document::open(bytes.clone()) {
                    Ok(doc) => doc,
                    Err(e) => {
                        emit!("ROW\ts{si}\t{name}\t{label}\tOPEN-ERR {e:?}");
                        continue;
                    }
                };
                let c = conservation(&bytes, &doc);
                let l = conservation_in_logical_order(&bytes, &doc);
                rows += 1;
                if !c.holds() || !l.holds() {
                    fails += 1;
                }
                let text: String = (0..doc.page_count())
                    .filter_map(|p| doc.page(p))
                    .map(|p| p.text().plain_text())
                    .collect::<Vec<_>>()
                    .join("|");
                emit!(
                    "ROW\ts{si}\t{name}\t{label}\tcontent={}\tlogical={}\t{:?}\t{}",
                    c.holds(),
                    l.holds(),
                    l.divergences.first().or(c.divergences.first()),
                    if text.chars().count() < 400 {
                        esc(&text)
                    } else {
                        "<long>".to_owned()
                    }
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
