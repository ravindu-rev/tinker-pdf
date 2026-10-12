//! Scratch round-2 review probe (never committed): K-11's neighbours in books
//! the fix was not tuned on. One ROW per book.
#![allow(missing_docs, clippy::all)]

mod epub_support;


mod zz_vov_shim {
    use tinker_pdf::{Document, TextOptions};
    use tinker_pdf_shape::unicode::{bidi_class, BidiClass};
    fn h(s: &str) -> u64 {
        let mut x: u64 = 0xcbf29ce484222325;
        for b in s.bytes() {
            x ^= u64::from(b);
            x = x.wrapping_mul(0x100000001b3);
        }
        x
    }
    pub fn styled_book(lang: &str, style: &str, body: &str) -> Vec<u8> {
        let bytes = super::epub_support::book::styled_book(lang, style, body);
        let mut vov = 0usize;
        let mut at = String::new();
        if let Ok(doc) = Document::open(bytes.clone()) {
            for p in 0..doc.page_count() {
                let Some(page) = doc.page(p) else { continue };
                let drawn = page.text_with(&TextOptions { content_order: true });
                let mut boxes: Vec<(i64, f64, f64, f64, String)> = Vec::new();
                for line in drawn.lines() {
                    for c in &line.chars {
                        let (x0, _, x1, _) = c.quad.bounds();
                        if c.text.chars().next().is_some_and(|ch| bidi_class(ch) == BidiClass::NSM) {
                            continue;
                        }
                        if !c.text.trim().is_empty() {
                            boxes.push(((c.origin.1 * 10.0).round() as i64, x0, x1, c.size, c.text.clone()));
                        }
                    }
                }
                boxes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
                for pair in boxes.windows(2) {
                    let (a, b) = (&pair[0], &pair[1]);
                    if a.0 == b.0 && b.1 < a.2 - 0.02 * b.3 {
                        vov += 1;
                        if at.is_empty() {
                            at = format!("{:?}@{:.2}..{:.2}/{:?}@{:.2}", a.4, a.1, a.2, b.4, b.1);
                        }
                    }
                }
            }
        }
        println!("VOVX\t{style}{}\t{lang}\t{:016x}\t{vov}\t{at}", if body.contains("word-spacing") || body.contains("justify") { " INLINE-WS" } else { "" }, h(&format!("{lang}|{body}")));
        bytes
    }
}
use zz_vov_shim::styled_book;
use epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::Document;

fn strip(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0591}'..='\u{05C7}'))
        .collect()
}

const AR: [&str; 8] = ["عوامل", "عدّة", "وقد", "أصبحنا", "نعرف", "الآن", "الموادّ", "تكبح"];
const HE: [&str; 8] = ["שָׁלוֹם", "עוֹלָם", "מִלְחָמָה", "סֵפֶר", "בַּיִת", "גָּדוֹל", "אוֹר", "יוֹם"];

fn books() -> Vec<(String, &'static str, String)> {
    let mut out = Vec::new();
    for (script, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        let rtl = |name: &str, text: String, out: &mut Vec<(String, &'static str, String)>| {
            out.push((format!("{script}/rtl/{name}"), lang, format!("<p dir=\"rtl\">{text}</p>")));
            out.push((format!("{script}/none/{name}"), lang, format!("<p>{text}</p>")));
            out.push((format!("{script}/css/{name}"), lang, format!("<div style=\"direction: rtl\"><p>{text}</p></div>")));
        };
        rtl("cite", format!("{} {} (Smith, 2010). {} {} {} (Jones et al., 2012). {} {}.", s(0), s(1), s(2), s(3), s(4), s(5), s(6)), &mut out);
        rtl("pages", format!("{} {} (p. 23) {} {} (pp. 4–5) {} {}.", s(0), s(1), s(2), s(3), s(4), s(5)), &mut out);
        rtl("eg", format!("{} {} (e.g., HDAC1 and HDAC2) {} {} {} {}.", s(0), s(1), s(2), s(3), s(4), s(5)), &mut out);
        rtl("indic", format!("{} {} (٢٠١٠). {} {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("double", format!("{} {} ((HDAC)). {} {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("braces", format!("{} {} {{HDAC}}. {} {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("angle", format!("{} {} ⟨HDAC⟩. {} {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("pairs-adjacent", format!("{} {} (HDAC)(SIRT). {} {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("pairs-two", format!("{} {} (HDAC) (SIRT). {} {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("square-in-round", format!("{} {} (HDAC [class I]). {} {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("pct", format!("{} {} (5%) {} (-3.5) {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("money", format!("{} {} ($1,234.56). {} {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("range", format!("{} {} (2010–2015). {} {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("mixed-inside", format!("{} {} (HDAC {}). {} {} {}.", s(0), s(1), s(2), s(3), s(4), s(5)), &mut out);
        rtl("mixed-inside-b", format!("{} {} ({} HDAC). {} {} {}.", s(0), s(1), s(2), s(3), s(4), s(5)), &mut out);
        rtl("enum", format!("{} (a) {} (b) {} (c) {}.", s(0), s(1), s(2), s(3)), &mut out);
        rtl("bold-latin", format!("{} {} (<b>HDAC</b>). {} {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("bold-bracket", format!("{} {} <b>(HDAC)</b>. {} {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("italic-run", format!("{} {} <i>(histone deacetylase)</i>. {} {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("span-ltr", format!("{} {} <span dir=\"ltr\">(HDAC)</span>. {} {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("bdi", format!("{} {} (<bdi>HDAC</bdi>). {} {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        rtl("colon", format!("{}: (HDAC) {}; (SIRT) {}.", s(0), s(1), s(2)), &mut out);
        rtl("dash", format!("{} - (HDAC) - {} {}.", s(0), s(1), s(2)), &mut out);
        rtl("url", format!("{} https://example.org/a_(b) {} {}.", s(0), s(1), s(2)), &mut out);
        rtl("emoticon", format!("{} :-) {} (HDAC) {} ;-) {}.", s(0), s(1), s(2), s(3)), &mut out);
        rtl("list-he", format!("א) {} {} ב) {} (HDAC) {} ג) {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        // Left-to-right paragraphs.
        out.push((format!("{script}/en/intervals"), "en", format!("<p>Intervals [0, 1) and (2, 3] hold {} here, and the text goes on for a while to wrap.</p>", s(0))));
        out.push((format!("{script}/en/func"), "en", format!("<p>See f(x) = {}(y) for details, and g(x) = {} (z) in the next part of the text.</p>", s(0), s(1))));
        out.push((format!("{script}/en/enum"), "en", format!("<p>The list: (a) {}, (b) {}, (c) {} and (d) the end of the list of words.</p>", s(0), s(1), s(2))));
        out.push((format!("{script}/en/roman"), "en", format!("<p>Item i) {} ii) {} (note) iii) end of items and iv) more words after it.</p>", s(0), s(1))));
        out.push((format!("{script}/en/emoticon"), "en", format!("<p>Smile :-) and {} ;-) then (wink) {} and more words to wrap the line here.</p>", s(0), s(1))));
        out.push((format!("{script}/en/chem"), "en", format!("<p>Chemistry (CH3)2 {} (aq) and math (a+b)(c+d) = {} hold here too.</p>", s(0), s(1))));
        out.push((format!("{script}/en/cite"), "en", format!("<p>Smith (2010) and {} (2011) say [12], [13], and {} [14] agrees with them.</p>", s(0), s(1))));
        out.push((format!("{script}/en/spaced"), "en", format!("<p>Note ( {} ) spaced (x) and ( {} {} ) spaced again in the line.</p>", s(0), s(1), s(2))));
        out.push((format!("{script}/en/rtl-pair-run"), "en", format!("<p>He wrote: {} ({} {}) and then ({}) in the margin of the page.</p>", s(0), s(1), s(2), s(3))));
        out.push((format!("{script}/en/dirless-rtl-ends"), lang, format!("<p>{} {} (HDAC) {} {} (SIRT) {} {}</p>", s(0), s(1), s(2), s(3), s(4), s(5))));
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

#[test]
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
                        println!("ROW\ts{si}\t{name}\t{label}\tOPEN-ERR {e:?}");
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
                println!(
                    "ROW\ts{si}\t{name}\t{label}\tcontent={}\tlogical={}\t{:?}\t{}",
                    c.holds(),
                    l.holds(),
                    l.divergences.first().or(c.divergences.first()),
                    text
                );
            }
        }
    }
    println!("rows={rows} fails={fails}");
}
