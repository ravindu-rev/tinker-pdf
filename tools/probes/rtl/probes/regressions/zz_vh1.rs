//! Verification of reviewer hunt1's findings (scratch, never committed).
#![allow(missing_docs, clippy::all, unused)]

mod epub_support;

use epub_support::book::styled_book;
use epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::{Document, TextOptions};

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Block texts of the body (p, td, br split).
fn segments(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_tag = false;
    let mut tag = String::new();
    for c in body.chars() {
        if in_tag {
            if c == '>' {
                in_tag = false;
                let name = tag
                    .trim_start_matches('/')
                    .split(|c: char| c.is_whitespace() || c == '/')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if ["p", "td", "tr", "table", "br", "div"].contains(&name.as_str()) {
                    let n = norm(&cur);
                    if !n.is_empty() {
                        out.push(n);
                    }
                    cur.clear();
                }
                tag.clear();
            } else {
                tag.push(c);
            }
        } else if c == '<' {
            in_tag = true;
        } else {
            cur.push(c);
        }
    }
    let n = norm(&cur);
    if !n.is_empty() {
        out.push(n);
    }
    out
}

fn tokens(segs: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in segs {
        for t in s.split(|c: char| !(c.is_alphanumeric() || matches!(c, '\u{0591}'..='\u{05C7}' | '\u{064B}'..='\u{065F}'))) {
            if t.chars().filter(|c| c.is_alphanumeric()).count() >= 2 && !out.iter().any(|o| o == t) {
                out.push(t.to_owned());
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy)]
struct M {
    cons: bool,
    logical: bool,
    whole: bool,
    found: usize,
    toks: usize,
}

fn measure(tag: &str, lang: &str, style: &str, body: &str) -> M {
    let bytes = styled_book(lang, style, body);
    let doc = Document::open(bytes.clone()).expect("opens");
    let mut text = String::new();
    let mut texts = Vec::new();
    for at in 0..doc.page_count() {
        let page = doc.page(at).expect("page");
        let t = page.text();
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&t.plain_text());
        texts.push(t);
    }
    let full = norm(&text);
    let segs = segments(body);
    let whole = segs.iter().all(|s| full.contains(s.as_str()));
    let toks = tokens(&segs);
    let found = toks.iter().filter(|t| texts.iter().any(|x| !x.search(t).is_empty())).count();
    let c = conservation(&bytes, &doc);
    let l = conservation_in_logical_order(&bytes, &doc);
    let m = M { cons: c.holds(), logical: l.holds(), whole, found, toks: toks.len() };
    println!(
        "VROW\t{tag}\tcons={}\tlogical={}\twhole={}\tsearch={}/{}\ttext={}\tc0={:?}\tl0={:?}",
        m.cons as u8,
        m.logical as u8,
        m.whole as u8,
        m.found,
        m.toks,
        text.replace('\n', " | "),
        c.divergences.first(),
        l.divergences.first(),
    );
    m
}

// ---- H1: the final stop after a number in a dir-less / ltr Hebrew paragraph.
#[test]
fn h1_final_stop_after_a_number() {
    let ws = "p { word-spacing: 0.5em }";
    let mut ms = Vec::new();
    ms.push(measure("h1/min/none", "he", ws, "<p>ש</p><p>בראשית. אור ٦٠.</p>"));
    ms.push(measure("h1/min/ltr", "he", ws, "<p>ש</p><p dir=\"ltr\">בראשית. אור ٦٠.</p>"));
    ms.push(measure("h1/min-en/none", "he", ws, "<p>ש</p><p>בראשית. אור 60.</p>"));
    ms.push(measure("h1/min-colon/none", "he", ws, "<p>ש</p><p>בראשית. אור ٦٠:</p>"));
    ms.push(measure(
        "h1/ofscripts-hen-prose-nat-none",
        "he",
        "p { word-spacing: 0.5em; margin: 0 20% }",
        "<p>שלום ١٢٣ מלחמה, ۴۵ בית ٢٠١٠. ۴۵ בית ٢٠١٠, אור ۷۸۹ בראשית. אור ۷۸۹ בראשית, ٣٫٥ אלהים ٦٠.</p>",
    ));
    ms.push(measure(
        "h1/breadth-A-he-en-dot-pref-none",
        "he",
        "p, td { word-spacing: 0.5em }",
        "<p>ש</p><p>בראשית ברא. אור גדול 60.</p>",
    ));
    // Controls: no number.
    let ctl = measure("h1/ctl-no-number", "he", ws, "<p>ש</p><p>בראשית. אור.</p>");
    // Real-world-ish: no single-letter preface, the paragraph's own first letter is the code-32 one.
    measure("h1/noprefix", "he", ws, "<p>בראשית. אור ٦٠.</p>");
    let bad: Vec<usize> = ms.iter().enumerate().filter(|(_, m)| !m.cons).map(|(i, _)| i).collect();
    assert!(bad.is_empty(), "H1 books not conserving: {bad:?}");
    assert!(ctl.cons);
}

// ---- H2: a Greek or Cyrillic cell before a right-to-left cell in a dir-less table.
#[test]
fn h2_overflow_cell_before_rtl_cell() {
    let mut ms = Vec::new();
    ms.push(measure("h2/min-1em", "he", "td { word-spacing: 1em }", "<table><tr><td>αβ γδ</td><td>ספר</td></tr></table>"));
    ms.push(measure("h2/min-0.6em", "he", "td { word-spacing: 0.6em }", "<table><tr><td>αβ γδ</td><td>ספר</td></tr></table>"));
    ms.push(measure(
        "h2/alpha-pointed",
        "he",
        "td { word-spacing: 1em }",
        "<table><tr><td>αλφα βήτα</td><td>סֵפֶר בַּיִת</td></tr></table>",
    ));
    ms.push(measure(
        "h2/cyr-ar",
        "ar",
        "p, td { word-spacing: 1em }",
        "<table><tr><td>привет мир</td><td>كتاب</td></tr></table>",
    ));
    ms.push(measure(
        "h2/breadth-B-gr-he-0-one",
        "he",
        "p, td { word-spacing: 1em }",
        "<table><tr><td>αλφα βήτα</td><td>שלום</td></tr></table>",
    ));
    let bad: Vec<usize> = ms.iter().enumerate().filter(|(_, m)| !m.cons).map(|(i, _)| i).collect();
    assert!(bad.is_empty(), "H2 books not conserving: {bad:?}");
}

// ---- H3: V3's one text object.
#[test]
fn h3_v3_one_text_object() {
    let just = "p { letter-spacing: 0.1em; word-spacing: 0.25em; text-align: justify; margin: 0 35% }";
    let mut cons_books = Vec::new();
    cons_books.push(measure("h3a/acomma", "he", just, "<p>שלום בראשית 25، <b>עולם</b> מלחמה.</p>"));
    cons_books.push(measure("h3a/angle", "he", just, "<p>שלום בראשית ⟨25⟩ <b>עולם</b> מלחמה.</p>"));
    cons_books.push(measure("h3b/pct-rtl", "ar", "p, td { word-spacing: 1em }", "<p dir=\"rtl\">جميل بيت 25٪ <b>كبير</b> نور.</p>"));
    cons_books.push(measure("h3b/shekel-rtl", "ar", "p, td { word-spacing: 1em }", "<p dir=\"rtl\">جميل بيت 25₪ <b>كبير</b> نور.</p>"));
    let lg = measure("h3c/pct-none-logical", "ar", "p { word-spacing: 1em }", "<p>كتاب عوامل 25٪ <b>مدرسة</b> الطالب.</p>");
    // C3's own book shape, for reference.
    measure("h3/v3-own", "ar", "p { word-spacing: 0.5em; margin: 0 20% }", "<p>مدرسة الطالب جميل بيت ٥٪ <b>كبير</b> نور</p>");
    let bad: Vec<usize> = cons_books.iter().enumerate().filter(|(_, m)| !m.cons).map(|(i, _)| i).collect();
    assert!(bad.is_empty() && lg.logical, "H3 books failing: cons {bad:?}, logical {}", lg.logical);
}

// ---- H4: baseline (a) pre-existing losses.
#[test]
fn h4_preexisting() {
    let a = measure("h4a/pct-2-rtl", "ar", "p, td { word-spacing: 1em }", "<p dir=\"rtl\">نور أصبحنا 25٪ <b>نعرف</b> كتاب.</p>");
    let a2 = measure("h4a/pct-2-none", "ar", "p, td { word-spacing: 1em }", "<p>نور أصبحنا 25٪ <b>نعرف</b> كتاب.</p>");
    let b = measure(
        "h4b/hen-prose-out-rtl",
        "he",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify; margin: 0 25% }",
        "<p>ף</p><p dir=\"rtl\">שלום ١٢٣ מלחמה, ۴۵ בית ٢٠١٠. ۴۵ בית ٢٠١٠, אור ۷۸۹ בראשית. אור ۷۸۹ בראשית, ٣٫٥ אלהים ٦٠.</p>",
    );
    assert!(a.cons && a2.cons && b.whole, "H4 failing");
}

// ---- H6: mixed stop runs after a <br/>.
#[test]
fn h6_mixed_stop_runs() {
    let mut ms = Vec::new();
    ms.push(measure("h6/ar-dot-ex", "ar", "", "<p dir=\"rtl\">كتاب عوامل<br/>.!<b>(</b>جميل<b>)</b> بيت نعرف</p>"));
    ms.push(measure("h6/ar-ex-ell", "ar", "", "<p dir=\"rtl\">كتاب عوامل<br/>!…<b>(</b>جميل<b>)</b> بيت نعرف</p>"));
    ms.push(measure("h6/he-ex-ell", "he", "", "<p dir=\"rtl\">שלום בראשית<br/>!…<b>(</b>ספר<b>)</b> בית הארץ</p>"));
    ms.push(measure("h6/he-dot-ex", "he", "", "<p dir=\"rtl\">שלום בראשית<br/>.!<b>(</b>ספר<b>)</b> בית הארץ</p>"));
    ms.push(measure(
        "h6/ar-dot-ex-0.6em",
        "ar",
        "p { word-spacing: 0.6em }",
        "<p dir=\"rtl\">كتاب عوامل<br/>.!<b>(</b>جميل<b>)</b> بيت نعرف</p>",
    ));
    // Priced controls.
    measure("h6/ctl-ell", "ar", "", "<p dir=\"rtl\">كتاب عوامل<br/>…<b>(</b>جميل<b>)</b> بيت نعرف</p>");
    measure("h6/ctl-qe", "ar", "", "<p dir=\"rtl\">كتاب عوامل<br/>?!<b>(</b>جميل<b>)</b> بيت نعرف</p>");
    measure("h6/ctl-ee", "ar", "", "<p dir=\"rtl\">كتاب عوامل<br/>!!<b>(</b>جميل<b>)</b> بيت نعرف</p>");
    measure("h6/ctl-dot", "ar", "", "<p dir=\"rtl\">كتاب عوامل<br/>.<b>(</b>جميل<b>)</b> بيت نعرف</p>");
    let bad: Vec<usize> = ms.iter().enumerate().filter(|(_, m)| !m.cons).map(|(i, _)| i).collect();
    assert!(bad.is_empty(), "H6 books not conserving: {bad:?}");
}
