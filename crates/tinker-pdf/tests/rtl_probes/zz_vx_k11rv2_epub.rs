//! `zz_vx_k11rv2_epub` (vx, round-5 review (confirm)): the books of
//! `tools/probes/rtl/probes/pdf/zz_vx_k11rv2_epub.rs`, which is the probe as it ran.
//!
//! Round-2 review probe: K-11's neighbours in books
//! the fix was not tuned on. One ROW per book.

use super::common::strip;
use super::vov::styled_book;
use crate::epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::Document;

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

fn books() -> Vec<(String, &'static str, String)> {
    let mut out = Vec::new();
    for (script, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        let rtl = |name: &str, text: String, out: &mut Vec<(String, &'static str, String)>| {
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
            out.push((
                format!("{script}/css/{name}"),
                lang,
                format!("<div style=\"direction: rtl\"><p>{text}</p></div>"),
            ));
        };
        rtl(
            "cite",
            format!(
                "{} {} (Smith, 2010). {} {} {} (Jones et al., 2012). {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4),
                s(5),
                s(6)
            ),
            &mut out,
        );
        rtl(
            "pages",
            format!(
                "{} {} (p. 23) {} {} (pp. 4–5) {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4),
                s(5)
            ),
            &mut out,
        );
        rtl(
            "eg",
            format!(
                "{} {} (e.g., HDAC1 and HDAC2) {} {} {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4),
                s(5)
            ),
            &mut out,
        );
        rtl(
            "indic",
            format!("{} {} (٢٠١٠). {} {} {}.", s(0), s(1), s(2), s(3), s(4)),
            &mut out,
        );
        rtl(
            "double",
            format!("{} {} ((HDAC)). {} {} {}.", s(0), s(1), s(2), s(3), s(4)),
            &mut out,
        );
        rtl(
            "braces",
            format!("{} {} {{HDAC}}. {} {} {}.", s(0), s(1), s(2), s(3), s(4)),
            &mut out,
        );
        rtl(
            "angle",
            format!("{} {} ⟨HDAC⟩. {} {} {}.", s(0), s(1), s(2), s(3), s(4)),
            &mut out,
        );
        rtl(
            "pairs-adjacent",
            format!(
                "{} {} (HDAC)(SIRT). {} {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        rtl(
            "pairs-two",
            format!(
                "{} {} (HDAC) (SIRT). {} {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        rtl(
            "square-in-round",
            format!(
                "{} {} (HDAC [class I]). {} {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        rtl(
            "pct",
            format!("{} {} (5%) {} (-3.5) {} {}.", s(0), s(1), s(2), s(3), s(4)),
            &mut out,
        );
        rtl(
            "money",
            format!("{} {} ($1,234.56). {} {} {}.", s(0), s(1), s(2), s(3), s(4)),
            &mut out,
        );
        rtl(
            "range",
            format!("{} {} (2010–2015). {} {} {}.", s(0), s(1), s(2), s(3), s(4)),
            &mut out,
        );
        rtl(
            "mixed-inside",
            format!(
                "{} {} (HDAC {}). {} {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4),
                s(5)
            ),
            &mut out,
        );
        rtl(
            "mixed-inside-b",
            format!(
                "{} {} ({} HDAC). {} {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4),
                s(5)
            ),
            &mut out,
        );
        rtl(
            "enum",
            format!("{} (a) {} (b) {} (c) {}.", s(0), s(1), s(2), s(3)),
            &mut out,
        );
        rtl(
            "bold-latin",
            format!(
                "{} {} (<b>HDAC</b>). {} {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        rtl(
            "bold-bracket",
            format!(
                "{} {} <b>(HDAC)</b>. {} {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        rtl(
            "italic-run",
            format!(
                "{} {} <i>(histone deacetylase)</i>. {} {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        rtl(
            "span-ltr",
            format!(
                "{} {} <span dir=\"ltr\">(HDAC)</span>. {} {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        rtl(
            "bdi",
            format!(
                "{} {} (<bdi>HDAC</bdi>). {} {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        rtl(
            "colon",
            format!("{}: (HDAC) {}; (SIRT) {}.", s(0), s(1), s(2)),
            &mut out,
        );
        rtl(
            "dash",
            format!("{} - (HDAC) - {} {}.", s(0), s(1), s(2)),
            &mut out,
        );
        rtl(
            "url",
            format!("{} https://example.org/a_(b) {} {}.", s(0), s(1), s(2)),
            &mut out,
        );
        rtl(
            "emoticon",
            format!("{} :-) {} (HDAC) {} ;-) {}.", s(0), s(1), s(2), s(3)),
            &mut out,
        );
        rtl(
            "list-he",
            format!(
                "א) {} {} ב) {} (HDAC) {} ג) {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        // Left-to-right paragraphs.
        out.push((format!("{script}/en/intervals"), "en", format!("<p>Intervals [0, 1) and (2, 3] hold {} here, and the text goes on for a while to wrap.</p>", s(0))));
        out.push((format!("{script}/en/func"), "en", format!("<p>See f(x) = {}(y) for details, and g(x) = {} (z) in the next part of the text.</p>", s(0), s(1))));
        out.push((
            format!("{script}/en/enum"),
            "en",
            format!(
                "<p>The list: (a) {}, (b) {}, (c) {} and (d) the end of the list of words.</p>",
                s(0),
                s(1),
                s(2)
            ),
        ));
        out.push((
            format!("{script}/en/roman"),
            "en",
            format!(
                "<p>Item i) {} ii) {} (note) iii) end of items and iv) more words after it.</p>",
                s(0),
                s(1)
            ),
        ));
        out.push((
            format!("{script}/en/emoticon"),
            "en",
            format!(
                "<p>Smile :-) and {} ;-) then (wink) {} and more words to wrap the line here.</p>",
                s(0),
                s(1)
            ),
        ));
        out.push((
            format!("{script}/en/chem"),
            "en",
            format!(
                "<p>Chemistry (CH3)2 {} (aq) and math (a+b)(c+d) = {} hold here too.</p>",
                s(0),
                s(1)
            ),
        ));
        out.push((
            format!("{script}/en/cite"),
            "en",
            format!(
                "<p>Smith (2010) and {} (2011) say [12], [13], and {} [14] agrees with them.</p>",
                s(0),
                s(1)
            ),
        ));
        out.push((
            format!("{script}/en/spaced"),
            "en",
            format!(
                "<p>Note ( {} ) spaced (x) and ( {} {} ) spaced again in the line.</p>",
                s(0),
                s(1),
                s(2)
            ),
        ));
        out.push((
            format!("{script}/en/rtl-pair-run"),
            "en",
            format!(
                "<p>He wrote: {} ({} {}) and then ({}) in the margin of the page.</p>",
                s(0),
                s(1),
                s(2),
                s(3)
            ),
        ));
        out.push((
            format!("{script}/en/dirless-rtl-ends"),
            lang,
            format!(
                "<p>{} {} (HDAC) {} {} (SIRT) {} {}</p>",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4),
                s(5)
            ),
        ));
        // A table of right-to-left cells, close together.
        out.push((format!("{script}/table/rtl"), lang, format!(
            "<table dir=\"rtl\" style=\"border-spacing: 2px\"><tr><td>{}</td><td>(HDAC)</td><td>{}.</td><td>{}</td></tr><tr><td>{} {}</td><td>{}</td><td>(2010)</td><td>x</td></tr></table>",
            s(0), s(1), s(2), s(3), s(4), s(5))));
        out.push((format!("{script}/table/ltr"), lang, format!(
            "<table style=\"border-spacing: 2px\"><tr><td>{}</td><td>(HDAC)</td><td>{}.</td><td>{}</td></tr><tr><td>{} {}</td><td>{}</td><td>(2010)</td><td>x</td></tr></table>",
            s(0), s(1), s(2), s(3), s(4), s(5))));
    }
    out
}

fn probe() {
    let styles = [
        "",
        "p { text-align: justify; margin: 0 30% }",
        "p { margin: 0 38% }",
        "p { word-spacing: 0.6em; margin: 0 20% }",
        "p, td { letter-spacing: 0.5em }",
        "p, td { letter-spacing: 0.3em; text-align: justify; margin: 0 25% }",
        "body { font-family: sans-serif } p { text-align: justify; margin: 0 28% }",
        "body { font-family: monospace } p { margin: 0 22% }",
    ];
    let (mut rows, mut fails) = (0, 0);
    for (si, style) in styles.iter().enumerate() {
        for (name, lang, body) in books() {
            for (label, body) in [("pointed", body.clone()), ("twin", strip(&body))] {
                if label == "twin" && body == strip(&body) {
                    // nothing to strip in an unpointed book; still a row
                }
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
                    .join("|")
                    .replace('\n', "|");
                emit!(
                    "ROW\ts{si}\t{name}\t{label}\tcontent={}\tlogical={}\t{:?}\t{}",
                    c.holds(),
                    l.holds(),
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
