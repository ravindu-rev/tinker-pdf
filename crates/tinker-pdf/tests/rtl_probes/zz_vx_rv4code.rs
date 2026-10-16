//! `zz_vx_rv4code` (vx, round-5 review (confirm)): the books of
//! `tools/probes/rtl/probes/pdf/zz_vx_rv4code.rs`, which is the probe as it ran.
//!
//! Round-4 code review probe: the characters the
//! `reads_right_to_left` table takes beyond `R`/`AL` (ornate parentheses,
//! Arabic number punctuation), palindromic neutral slices under word
//! spacing, and numbers/Latin words inside word-spaced right-to-left text.
//! One ROW per book: content, logical, search.

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

const OPEN: char = '\u{FD3F}';

const CLOSE: char = '\u{FD3E}';

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
        };
        // Ornate parentheses, FD3F opening and FD3E closing in RTL use, and swapped.
        for (tag, o, c) in [("orn", OPEN, CLOSE), ("orn-sw", CLOSE, OPEN)] {
            para(
                &format!("{tag}-i"),
                format!("<i>{o}</i>{} {}<i>{c}</i>", s(0), s(1)),
                &mut out,
            );
            para(
                &format!("{tag}-b-mid"),
                format!(
                    "{} {} <b>{o}</b>{} {}<b>{c}</b> {} {}.",
                    s(2),
                    s(3),
                    s(0),
                    s(1),
                    s(4),
                    s(5)
                ),
                &mut out,
            );
            para(
                &format!("{tag}-plain"),
                format!(
                    "{} {} {o}{} {}{c} {} {}.",
                    s(2),
                    s(3),
                    s(0),
                    s(1),
                    s(4),
                    s(5)
                ),
                &mut out,
            );
            para(
                &format!("{tag}-x3"),
                (0..3)
                    .map(|i| {
                        format!(
                            "{} {} {o}{} {}{c} {} {} {}. ",
                            s(i),
                            s(i + 1),
                            s(i + 2),
                            s(i + 3),
                            s(i + 4),
                            s(i + 5),
                            s(i + 6)
                        )
                    })
                    .collect(),
                &mut out,
            );
            para(
                &format!("{tag}-latin"),
                format!("{} {o}HDAC{c} {} {}.", s(0), s(1), s(2)),
                &mut out,
            );
            out.push((
                format!("{script}/en/{tag}"),
                "en",
                format!("<p>The verse {o}{} {}{c} and the words {o}<i>{}</i>{c} are quoted in this English sentence here.</p>", s(0), s(1), s(2)),
            ));
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((
                    format!("{script}/table-{tag}/{d}"),
                    lang,
                    format!(
                        "<table{attr} style=\"border-spacing: 0\"><tr><td>{}</td><td>{o}{}{c}</td><td>{}</td></tr><tr><td>{o}{}{c}</td><td>{}</td><td>x</td></tr></table>",
                        s(0), s(1), s(2), s(3), s(4)
                    ),
                ));
            }
        }
        // Arabic number punctuation: thousands and decimal separators, percent, per mille.
        para(
            "an-sep",
            format!("{} {} (١٬٢٣٤) {} ٣٫١٤ {} {}.", s(0), s(1), s(2), s(3), s(4)),
            &mut out,
        );
        para(
            "an-pct",
            format!("{} ٥٠٪ {} <i>٪</i> {} ؉ {}.", s(0), s(1), s(2), s(3)),
            &mut out,
        );
        para(
            "an-sep-i",
            format!("{} <i>٬</i>{} <b>٫</b> {}.", s(0), s(1), s(2)),
            &mut out,
        );
        para(
            "an-sep-x3",
            (0..3)
                .map(|i| {
                    format!(
                        "{} {} {} (١٬٢٣٤), {} {} {} {}. ",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4),
                        s(i + 5),
                        s(i + 6)
                    )
                })
                .collect(),
            &mut out,
        );
        for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
            out.push((
                format!("{script}/table-an/{d}"),
                lang,
                format!(
                    "<table{attr} style=\"border-spacing: 0\"><tr><td>{}</td><td>٪</td><td>{}</td></tr><tr><td>١٬٢٣٤</td><td>{}</td><td>٫</td></tr></table>",
                    s(0), s(1), s(2)
                ),
            ));
        }
        // Palindromic neutral slices after a word space.
        para(
            "pal-paren2",
            format!("{} <i>( (</i>{} {}.", s(0), s(1), s(2)),
            &mut out,
        );
        para(
            "pal-dash",
            format!("{} -- {} .. {} ## {}.", s(0), s(1), s(2), s(3)),
            &mut out,
        );
        para(
            "pal-hash",
            format!("{} #{} {} ##{} {}.", s(0), s(1), s(2), s(3), s(4)),
            &mut out,
        );
        para(
            "pal-mixed",
            format!("{} (#{}) {} [#]{} {}.", s(0), s(1), s(2), s(3), s(4)),
            &mut out,
        );
        // Numbers and Latin/Greek words inside right-to-left text.
        para(
            "num-paren",
            format!("(١٢٣) {} {} (١٢٣) {}.", s(0), s(1), s(2)),
            &mut out,
        );
        para(
            "num-en",
            format!("{} (2010) {} 15 {} 1,234.5 {}.", s(0), s(1), s(2), s(3)),
            &mut out,
        );
        para(
            "greek",
            format!("{} (αλφα βήτα) {} αλφα {}.", s(0), s(1), s(2)),
            &mut out,
        );
        para(
            "latin-2w",
            format!(
                "{} (histone deacetylase) {} HDAC SIRT {}.",
                s(0),
                s(1),
                s(2)
            ),
            &mut out,
        );
        para(
            "num-x3",
            (0..3)
                .map(|i| {
                    format!(
                        "{} {} {} (١٢٣), {} {} {} {}. ",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4),
                        s(i + 5),
                        s(i + 6)
                    )
                })
                .collect(),
            &mut out,
        );
        para(
            "greek-x3",
            (0..3)
                .map(|i| {
                    format!(
                        "{} {} (αλφα βήτα), {} {} {} {}. ",
                        s(i),
                        s(i + 1),
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
        "p, li { word-spacing: 1em; margin: 0 18% }",
        "p, li { word-spacing: 0.5em }",
        "body { font-family: monospace } p, li { text-align: justify; margin: 0 20% }",
        "body { font-family: sans-serif } p, li { text-align: justify; margin: 0 15% }",
        "td { padding: 0; letter-spacing: 0.6em } table { border-spacing: 0 }",
        "td { padding: 0; word-spacing: 1em; letter-spacing: 0.3em } table { border-spacing: 0 }",
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
                let keep = |c: char| {
                    c.is_alphanumeric()
                        || matches!(c, '\u{0591}'..='\u{05C7}' | '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{066B}' | '\u{066C}')
                };
                let words: Vec<String> = untag(&body)
                    .split_whitespace()
                    .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && !matches!(c, '\u{0591}'..='\u{05C7}' | '\u{064B}'..='\u{065F}' | '\u{0670}')).to_owned())
                    .filter(|w| w.chars().any(char::is_alphanumeric) && w.chars().all(keep))
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
