//! `zz_vx_k11rv3_own` (vx, round-5 review (confirm)): the books of
//! `tools/probes/rtl/probes/pdf/zz_vx_k11rv3_own.rs`, which is the probe as it ran.
//!
//! Round-3 review probe: what round 3 changed —
//! the direction of the word after a stretched gap, cell and block
//! boundaries on one baseline, chain joins, styled neutral runs at line
//! starts and ends, and Latin, numbers, Arabic and Hebrew in brackets and
//! quotation marks. One ROW per book: content, logical, search.

use super::common::{strip, untag, Book};
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

fn books() -> Vec<Book> {
    let mut out: Vec<Book> = Vec::new();
    for (script, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        let para = |name: &str, text: String, out: &mut Vec<Book>| {
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
        // The word after a gap: right-to-left words in runs of their own.
        para(
            "styled-words",
            format!(
                "{} <b>{}</b> <i>{}</i> {} <b>{}</b>.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        para(
            "styled-paren-latin",
            format!(
                "{} <b>(HDAC)</b> {} <i>⟨x⟩</i> {} {}.",
                s(0),
                s(1),
                s(2),
                s(3)
            ),
            &mut out,
        );
        para(
            "styled-quote-rtl",
            format!(
                "{} <b>«{}»</b> {} “{}” {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4),
                s(5)
            ),
            &mut out,
        );
        para(
            "styled-quote-latin",
            format!("{} <b>“HDAC”</b> {} «SIRT» {} {}.", s(0), s(1), s(2), s(3)),
            &mut out,
        );
        para(
            "bracket-rtl",
            format!(
                "{} <i>[{}]</i> {} ({}) {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4),
                s(5)
            ),
            &mut out,
        );
        para(
            "mixed-in-paren",
            format!(
                "{} (<b>{}</b> HDAC) {} {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        para(
            "rtl-then-latin-glued",
            format!("{} {}<b>abc</b> {} {} {}.", s(0), s(1), s(2), s(3), s(4)),
            &mut out,
        );
        para(
            "word-rtl-latin-mixed",
            format!("{} <b>{}abc</b> {} {} {}.", s(0), s(1), s(2), s(3), s(4)),
            &mut out,
        );
        para(
            "numbers-styled",
            format!(
                "{} 2010 <b>{}</b> 15% {} <i>٣٤</i> {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        para(
            "dashes-styled",
            format!(
                "{} <i>-</i> {} <i>–</i> HDAC <i>—</i> {} {}.",
                s(0),
                s(1),
                s(2),
                s(3)
            ),
            &mut out,
        );
        para(
            "latin-word-gap",
            format!(
                "{} {} <b>HDAC</b> <i>{}</i> {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        para(
            "two-latin",
            format!("{} HDAC SIRT <b>{}</b> {} {}.", s(0), s(1), s(2), s(3)),
            &mut out,
        );
        // Styled neutral runs at line starts and ends.
        para("open-qx", format!("<i>?!</i> {} {}", s(0), s(1)), &mut out);
        para("close-qx", format!("{} {} <i>?!</i>", s(0), s(1)), &mut out);
        para(
            "open-dots",
            format!("<b>...</b>{} {}", s(0), s(1)),
            &mut out,
        );
        para(
            "colon-b",
            format!("{}<b>:</b> {} {}", s(0), s(1), s(2)),
            &mut out,
        );
        para(
            "open-num",
            format!("<i>(1)</i> {} {}", s(0), s(1)),
            &mut out,
        );
        para(
            "guillemets-i",
            format!("<i>«</i>{} {}<i>»</i>", s(0), s(1)),
            &mut out,
        );
        para(
            "straight-q-b",
            format!("<b>\"</b>{} {}<b>\"</b>", s(0), s(1)),
            &mut out,
        );
        para(
            "close-bang",
            format!("{} {} <i>!</i>", s(0), s(1)),
            &mut out,
        );
        para("open-star", format!("<i>*</i> {} {}", s(0), s(1)), &mut out);
        para("hash", format!("<i>#</i>{} {}", s(0), s(1)), &mut out);
        para(
            "mid-neutral",
            format!(
                "{} <i>?!</i> {} <b>;</b> {} <i>…</i> {}",
                s(0),
                s(1),
                s(2),
                s(3)
            ),
            &mut out,
        );
        para(
            "long-neutral-mid",
            format!(
                "{} {} {} <i>?! -- ?!</i> {} {} {} {}.",
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
        para(
            "paren-styled-mid",
            format!(
                "{} {} <i>(</i>{} {}<i>)</i> {} {}.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4),
                s(5)
            ),
            &mut out,
        );
        // Latin, numbers, Arabic, Hebrew in brackets and quotation marks.
        for (tag, inside) in [
            ("hd", "histone deacetylase"),
            ("sq", "[HDAC]"),
            ("dq", "\"HDAC\""),
            ("gu", "«HDAC»"),
            ("cq", "“histone deacetylase”"),
            ("nest", "(HDAC [class I])"),
            ("see", "(see [HDAC] here)"),
            ("an", "(١٢٣)"),
            ("en", "(2010)"),
            ("rtl", "(@@)"),
            ("he-in-ar", "(שלום)"),
            ("ar-in-he", "(سلام)"),
            ("mixed", "(@@ 12 HDAC)"),
            ("q-rtl", "“@@”"),
        ] {
            let inside = inside.replace("@@", s(6));
            let inside = if inside.starts_with(['(', '[', '"', '«', '“']) {
                inside
            } else {
                format!("({inside})")
            };
            para(
                &format!("in-{tag}"),
                format!(
                    "{} {} {}. {} {} {} {} {}.",
                    s(0),
                    s(1),
                    inside,
                    s(2),
                    s(3),
                    s(4),
                    s(5),
                    s(7)
                ),
                &mut out,
            );
            para(
                &format!("in-{tag}-x3"),
                (0..3)
                    .map(|i| {
                        format!(
                            "{} {} {} {}, {} {} {} {}. ",
                            s(i),
                            s(i + 1),
                            s(i + 2),
                            inside,
                            s(i + 3),
                            s(i + 4),
                            s(i + 5),
                            s(i + 6)
                        )
                    })
                    .collect(),
                &mut out,
            );
        }
        // Left-to-right paragraphs quoting right-to-left text.
        let en = |name: &str, text: String, out: &mut Vec<Book>| {
            out.push((
                format!("{script}/en/{name}"),
                "en",
                format!("<p>{text}</p>"),
            ));
        };
        en("styled-gloss", format!("The word <b>{}</b> <i>{}</i> means peace, and (<b>{}</b>) too, with {} {} justified across the whole line here.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        en(
            "quotes",
            format!(
                "Note: «{} {}» and “{}” then [{}] with (1) {} and 2) end of the line.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        en(
            "gloss-paren",
            format!(
                "Peace ({} {}) and war ({} {} {}) are common words in the text we read today.",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4)
            ),
            &mut out,
        );
        en(
            "rtl-run-bold-mid",
            format!(
                "Here is a phrase: {} <b>{}</b> {} and it continues in English for a while longer.",
                s(0),
                s(1),
                s(2)
            ),
            &mut out,
        );
        en(
            "rtl-end-neutral",
            format!(
                "It ends with {} {}<i>?!</i> and then (HDAC) <b>{}</b>.",
                s(0),
                s(1),
                s(2)
            ),
            &mut out,
        );
        en(
            "latin-paren-rtl",
            format!(
                "The enzyme (HDAC) is {} {} and (SIRT) is {} in this sentence, written to wrap.",
                s(0),
                s(1),
                s(2)
            ),
            &mut out,
        );
        en(
            "numbered",
            format!(
                "Steps: 1) {} 2) <b>{}</b> 3) (x) {} and so on to the end.",
                s(0),
                s(1),
                s(2)
            ),
            &mut out,
        );
        // Tables: cells on one baseline, close together.
        for (ti, table_style) in [
            "border-spacing: 0",
            "border-spacing: 1px",
            "border-spacing: 2px",
            "border-spacing: 0; letter-spacing: 0.6em",
            "border-spacing: 0; letter-spacing: 0.55em",
            "border-spacing: 1px; letter-spacing: 0.75em",
            "border-spacing: 0; word-spacing: 1em",
        ]
        .iter()
        .enumerate()
        {
            let cells = [
                format!(
                    "<td>{}</td><td>{}</td><td>{}.</td><td>(HDAC)</td>",
                    s(0),
                    s(1),
                    s(2)
                ),
                format!(
                    "<td>{} {}</td><td>{}</td><td>(2010)</td><td>x</td>",
                    s(3),
                    s(4),
                    s(5)
                ),
                format!(
                    "<td>«{}»</td><td>{}</td><td>[{}]</td><td>{}</td>",
                    s(6),
                    s(7),
                    s(0),
                    s(1)
                ),
                format!(
                    "<td>abc</td><td>{}</td><td>def</td><td>{} {}</td>",
                    s(2),
                    s(3),
                    s(4)
                ),
            ];
            let rows: String = cells.iter().map(|r| format!("<tr>{r}</tr>")).collect();
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((
                    format!("{script}/table{ti}/{d}"),
                    lang,
                    format!("<table{attr} style=\"{table_style}\">{rows}</table>"),
                ));
                out.push((
                    format!("{script}/table{ti}-pad0/{d}"),
                    lang,
                    format!(
                        "<table{attr} style=\"{table_style}\">{}</table>",
                        cells
                            .iter()
                            .map(|r| format!(
                                "<tr>{}</tr>",
                                r.replace("<td>", "<td style=\"padding: 0\">")
                            ))
                            .collect::<String>()
                    ),
                ));
            }
        }
        // Inline blocks side by side, and lists.
        for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
            out.push((
                format!("{script}/inline-block/{d}"),
                lang,
                format!(
                    "<p{attr}><span style=\"display: inline-block\">{}</span><span style=\"display: inline-block\">{}</span> <span style=\"display: inline-block\">(HDAC)</span><span style=\"display: inline-block\">{}</span></p>",
                    s(0), s(1), s(2)
                ),
            ));
            out.push((
                format!("{script}/list/{d}"),
                lang,
                format!(
                    "<ul{attr}><li>{} <b>{}</b></li><li>(HDAC) {}</li><li><i>?!</i> {}</li></ul><ol{attr} style=\"list-style-position: inside\"><li>{} (SIRT)</li><li>«{}» {}</li></ol>",
                    s(0), s(1), s(2), s(3), s(4), s(5), s(6)
                ),
            ));
        }
    }
    out
}

fn probe() {
    let styles = [
        "",
        "p, li { text-align: justify; margin: 0 30% }",
        "p, li { text-align: justify; margin: 0 22% }",
        "p, li, td { letter-spacing: 0.5em }",
        "p, li, td { letter-spacing: 0.55em; text-align: justify; margin: 0 20% }",
        "p, li, td { letter-spacing: 0.3em; text-align: justify; margin: 0 25% }",
        "p, li, td { letter-spacing: 0.75em }",
        "p, li { word-spacing: 1em; margin: 0 18% }",
        "body { font-family: monospace } p, li { margin: 0 26% }",
        "body { font-family: sans-serif } p, li { text-align: justify; margin: 0 33% }",
        "p, li { margin: 0 40% }",
        "td { padding: 0; letter-spacing: 0.6em } table { border-spacing: 0 }",
    ];
    let (mut rows, mut fails) = (0, 0);
    for (si, style) in styles.iter().enumerate() {
        for (name, lang, body) in books() {
            for (label, body) in [("pointed", body.clone()), ("twin", strip(&body))] {
                let key = format!("s{si}\t{name}\t{label}");
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
                // Every word of letters or digits, its neutrals trimmed: found by search?
                let words: Vec<String> = untag(&body)
                    .split_whitespace()
                    .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && !matches!(c, '\u{0591}'..='\u{05C7}' | '\u{064B}'..='\u{065F}' | '\u{0670}')).to_owned())
                    .filter(|w| w.chars().any(char::is_alphanumeric) && w.chars().all(|c| c.is_alphanumeric() || matches!(c, '\u{0591}'..='\u{05C7}' | '\u{064B}'..='\u{065F}' | '\u{0670}')))
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
