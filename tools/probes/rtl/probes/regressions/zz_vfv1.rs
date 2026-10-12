//! Verification of reviewer fixverify1's findings (scratch, never committed).
//! Metrics are the hunt probes' own: cons, logical, text (read whole), search.
#![allow(missing_docs, clippy::all, unused)]

mod epub_support;

use epub_support::book::styled_book;
use epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::Document;

fn format_char(c: char) -> bool {
    matches!(c, '\u{AD}' | '\u{61C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

fn nsm(c: char) -> bool {
    matches!(c, '\u{0591}'..='\u{05BD}' | '\u{05BF}' | '\u{05C1}' | '\u{05C2}' | '\u{05C4}' | '\u{05C5}' | '\u{05C7}' | '\u{064B}'..='\u{065F}' | '\u{0670}')
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| !format_char(*c))
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

const BLOCKS: [&str; 26] = [
    "p", "li", "td", "th", "tr", "figcaption", "div", "br", "h1", "h2", "h3", "h4", "h5", "h6", "dt", "dd",
    "caption", "pre", "blockquote", "figure", "table", "ul", "ol", "dl", "section", "tbody",
];

fn segments(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = body.chars().peekable();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        let n = norm(cur);
        if !n.is_empty() {
            out.push(n);
        }
        cur.clear();
    };
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::new();
            while let Some(d) = chars.next() {
                if d == '>' {
                    break;
                }
                tag.push(d);
            }
            let name = tag
                .trim_start_matches('/')
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if BLOCKS.contains(&name.as_str()) {
                flush(&mut cur, &mut out);
            }
        } else if c == '\n' {
            flush(&mut cur, &mut out);
        } else {
            cur.push(c);
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn tokens(segs: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in segs {
        for t in s.split(|c: char| !(c.is_alphanumeric() || nsm(c))) {
            if t.chars().filter(|c| c.is_alphanumeric() && !nsm(*c)).count() >= 2 && !out.iter().any(|o| o == t) {
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
        "VROW\t{tag}\tcons={}\tlogical={}\ttext={}\tsearch={}/{}\t{}",
        m.cons as u8,
        m.logical as u8,
        m.whole as u8,
        m.found,
        m.toks,
        text.replace('\n', " | "),
    );
    m
}

fn rep(n: usize, f: impl Fn(usize) -> String) -> String {
    (0..n).map(|k| f(k * 3)).collect::<Vec<_>>().join(" ")
}

const HE: [&str; 12] = ["שלום", "עולם", "מלחמה", "ספר", "בית", "גדול", "אור", "יום", "בראשית", "ברא", "אלהים", "הארץ"];
const HEP: [&str; 12] = ["שָׁלוֹם", "עוֹלָם", "מִלְחָמָה", "סֵפֶר", "בַּיִת", "גָּדוֹל", "אוֹר", "יוֹם", "בְּרֵאשִׁית", "בָּרָא", "אֱלֹהִים", "הָאָרֶץ"];
const AR: [&str; 12] = ["كتاب", "مدرسة", "الطالب", "جميل", "بيت", "كبير", "نور", "يوم", "عوامل", "وقد", "أصبحنا", "نعرف"];
const HEN: [&str; 12] = ["שלום", "١٢٣", "מלחמה", "۴۵", "בית", "٢٠١٠", "אור", "۷۸۹", "בראשית", "٣٫٥", "אלהים", "٦٠"];

fn bundled() -> bool {
    cfg!(feature = "bundled-fonts")
}

fn check(name: &str, results: &[(String, bool)]) {
    let bad: Vec<&String> = results.iter().filter(|(_, ok)| !ok).map(|(t, _)| t).collect();
    println!("VSUM\t{name}\tbuild={}\tfailing={}/{}\t{:?}", if bundled() { "bundled" } else { "default" }, bad.len(), results.len(), bad);
    assert!(bad.is_empty(), "{name}: {} of {} books fail: {bad:?}", bad.len(), results.len());
}

// ---- R1-TABLECOST: dir-less table, `abc (x).` cell beside a right-to-left cell. Read whole.
#[test]
fn r1_tablecost_read_whole() {
    let mut r = Vec::new();
    let styles = [
        "table { border-spacing: 0 } td { padding: 0; word-spacing: 1em }",
        "table { border-spacing: 0 } td { padding: 0; letter-spacing: 0.1em; word-spacing: 0.5em }",
    ];
    let mut cells: Vec<(&str, &str, &str)> = vec![("neu-ar", "ar", "أصبحنا نعرف")];
    if bundled() {
        cells.push(("neu-he", "he", "ספר בית"));
    }
    for st in styles {
        for (n, lang, cell) in &cells {
            let body = format!("<table><tr><td>abc (x).</td><td>{cell}</td></tr><tr><td>{cell}</td><td>abc (x).</td></tr></table>");
            let tag = format!("r1/{n}/none [{st}]");
            let m = measure(&tag, lang, st, &body);
            r.push((tag, m.whole));
        }
    }
    check("r1_tablecost_read_whole", &r);
}

// ---- H1: dir-less / ltr Hebrew paragraph ending `<number><stop>` (default build). Conservation at B and M.
#[test]
fn h1_final_stop_conserves() {
    if bundled() {
        return;
    }
    let mut r = Vec::new();
    let st = "p { word-spacing: 0.5em; margin: 0 20% } td { word-spacing: 0.6em }";
    for (d, attr) in [("none", ""), ("ltr", " dir=\"ltr\"")] {
        let body = format!("<p>ש</p><p{attr}>בראשית ברא. אור גדול ٦٠.</p>");
        let tag = format!("h1/breadth-A-he-an-dot-pref-{d}");
        let m = measure(&tag, "he", st, &body);
        r.push((tag, m.cons));
    }
    let s = |i: usize| HEN[i % 12];
    let prose = rep(3, |i| format!("{} {} {}, {} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4), s(i + 5)));
    for (d, attr) in [("none", ""), ("ltr", " dir=\"ltr\"")] {
        let tag = format!("h1/ofscripts-hen-prose-nat-{d}");
        let m = measure(&tag, "he", "p { word-spacing: 0.5em; margin: 0 20% }", &format!("<p{attr}>{prose}</p>"));
        r.push((tag, m.cons));
    }
    check("h1_final_stop_conserves", &r);
}

// ---- DOC-H1-COUNT: the same shape at the monospace justified style, conserving at M only.
#[test]
fn h1_monospace_conserves_at_m_only() {
    if bundled() {
        return;
    }
    let mut r = Vec::new();
    let s = |i: usize| HEN[i % 12];
    let prose = rep(3, |i| format!("{} {} {}, {} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4), s(i + 5)));
    let st = "body { font-family: monospace } p { word-spacing: 0.5em; text-align: justify; margin: 0 25% }";
    for (d, attr) in [("none", ""), ("ltr", " dir=\"ltr\"")] {
        let tag = format!("h1m/ofscripts-hen-prose-nat-{d}");
        let m = measure(&tag, "he", st, &format!("<p{attr}>{prose}</p>"));
        r.push((tag, m.cons));
    }
    check("h1_monospace_conserves_at_m_only", &r);
}

// ---- CR1/F1: hunt spacing ar/styled/rtl at letter-spacing 0.45em justified (both builds): cons and text at B.
#[test]
fn f1_styled_neutrals_letter_spaced() {
    let s = |i: usize| AR[i % 12];
    let text = rep(6, |i| format!("{} <b>{}</b> <i>?!</i> {} <b>(</b>{}<b>)</b> {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)));
    let st = "p { letter-spacing: 0.45em; text-align: justify; margin: 0 32% }";
    let m = measure("f1/hunt-spacing-ar-styled-rtl", "ar", st, &format!("<p dir=\"rtl\">{text}</p>"));
    check("f1_styled_neutrals_letter_spaced", &[("f1/cons".into(), m.cons), ("f1/text".into(), m.whole)]);
}

// ---- CR1/F2: price/he/qe/b2/brb/rtl at letter-spacing 0.5em justified (default build): cons at B.
#[test]
fn f2_price_he_qe_b2_conserves() {
    if bundled() {
        return;
    }
    let m = measure(
        "f2/price-he-qe-b2-brb-rtl",
        "he",
        "p { letter-spacing: 0.5em; text-align: justify; margin: 0 25% }",
        "<p dir=\"rtl\">אור יום<br/>?!<b>{</b>בראשית<b>}</b> ברא אלהים</p>",
    );
    check("f2_price_he_qe_b2_conserves", &[("f2/cons".into(), m.cons)]);
}

// ---- CR1 vs M: mf/greek-he (default), gaps p1 (both), punct mid-ws (both): cons at M. gaps p8 (both): text at M.
#[test]
fn cr1_vs_m() {
    let mut r = Vec::new();
    if !bundled() {
        let m = measure("cr1/mf-greek-he", "en", "p { word-spacing: 0.5001em }", "<p>x αλφα שלום עולם y.</p>");
        r.push(("cr1/mf-greek-he cons".to_owned(), m.cons));
    }
    let s = |i: usize| AR[i % 12];
    let gap = |pi: usize, p: &str| rep(3, |i| format!("{} <i>{p}</i> {} <b>{p}</b>{} {}.", s(i + pi), s(i + pi + 1), s(i + pi + 2), s(i + pi + 3)));
    let m = measure("cr1/gaps-ar-p1-rtl", "ar", "p { word-spacing: 0.6em; margin: 0 20% }", &format!("<p dir=\"rtl\">{}</p>", gap(1, "!!")));
    r.push(("cr1/gaps-ar-p1-rtl cons".to_owned(), m.cons));
    let midws = rep(6, |i| {
        let x = s(i);
        let c: Vec<char> = x.chars().collect();
        let k = c.len() / 2;
        format!("{} <span style=\"word-spacing: 1em\">{} {}</span>{} ({}).", s(i + 1), s(i + 3), c[..k].iter().collect::<String>(), c[k..].iter().collect::<String>(), s(i + 2))
    });
    let m = measure("cr1/punct-ar-mid-ws-none", "ar", "", &format!("<p>{midws}</p>"));
    r.push(("cr1/punct-ar-mid-ws-none cons".to_owned(), m.cons));
    let m = measure(
        "cr1/gaps-ar-p8-rtl",
        "ar",
        "p { letter-spacing: 0.25em; text-align: justify; margin: 0 25% }",
        &format!("<p dir=\"rtl\">{}</p>", gap(8, ";")),
    );
    r.push(("cr1/gaps-ar-p8-rtl text".to_owned(), m.whole));
    check("cr1_vs_m", &r);
}

// ---- H3: C3's one text object, cons at M.
#[test]
fn h3_vs_m() {
    let mut r = Vec::new();
    let m = measure("h3/breadth-C-ar-pct-1-rtl", "ar", "p, td { word-spacing: 1em }", "<p dir=\"rtl\">جميل بيت 25٪ <b>كبير</b> نور.</p>");
    r.push(("h3/C-ar-pct-1-rtl".to_owned(), m.cons));
    let m = measure("h3/breadth-C-ar-shekel-1-rtl", "ar", "p, td { word-spacing: 1em }", "<p dir=\"rtl\">جميل بيت 25₪ <b>كبير</b> نور.</p>");
    r.push(("h3/C-ar-shekel-1-rtl".to_owned(), m.cons));
    if bundled() {
        let v3 = "p { letter-spacing: 0.1em; word-spacing: 0.25em; text-align: justify; margin: 0 35% }";
        let br = "p { letter-spacing: 0.1em; word-spacing: 0.25em; text-align: justify; margin: 0 35% } td { letter-spacing: 0.1em; word-spacing: 0.5em }";
        for (en, e) in [("acomma", "25\u{60C}"), ("angle", "\u{27E8}25\u{27E9}")] {
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                let tag = format!("h3/v3-he-en-{en}-bold-after-{d}");
                let m = measure(&tag, "he", v3, &format!("<p{attr}>שלום בראשית {e} <b>עולם</b> מלחמה.</p>"));
                r.push((tag, m.cons));
                // breadth C/he/{en}/0/{d}: s(0) s(8) e <b>s(1)</b> s(2).
                let tag = format!("h3/breadth-C-he-{en}-0-{d}");
                let m = measure(&tag, "he", br, &format!("<p{attr}>שלום בראשית {e} <b>עולם</b> מלחמה.</p>"));
                r.push((tag, m.cons));
            }
        }
    }
    check("h3_vs_m", &r);
}

// ---- H4/F3/F4: baseline (a), lost since M.
#[test]
fn h4_f3_f4_vs_b() {
    let mut r = Vec::new();
    for st in ["p, td { word-spacing: 1em }"] {
        for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
            let tag = format!("h4/C-ar-pct-2-{d} [{st}]");
            let m = measure(&tag, "ar", st, &format!("<p{attr}>نور أصبحنا 25٪ <b>نعرف</b> كتاب.</p>"));
            r.push((tag, m.cons));
        }
    }
    if !bundled() {
        let s = |i: usize| HEN[i % 12];
        let prose = rep(3, |i| format!("{} {} {}, {} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4), s(i + 5)));
        let tag = "h4/ofscripts-hen-prose-out-rtl".to_owned();
        let m = measure(
            &tag,
            "he",
            "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify; margin: 0 25% }",
            &format!("<p>ף</p><p dir=\"rtl\">{prose}</p>"),
        );
        r.push((tag, m.whole));
        // F4: zz_c1_tw hep/glued/none at the monospace justified style.
        let h8 = |i: usize| HEP[i % 8];
        let glued = (0..5)
            .map(|i| format!("{} <b>.</b>{} <i>?!</i> {} <b>(</b>{}<b>)</b> {};", h8(i), h8(i + 1), h8(i + 2), h8(i + 3), h8(i + 4)))
            .collect::<Vec<_>>()
            .join(" ");
        let tag = "f4/c1tw-hep-glued-none".to_owned();
        let m = measure(&tag, "he", "body { font-family: monospace } p { word-spacing: 0.5em; text-align: justify; margin: 0 25% }", &format!("<p>{glued}</p>"));
        r.push((tag, m.cons));
    } else {
        // F3: fv_extra c/hep/m6/b{0,1}/two/none at letter-spacing 0.5em justified.
        let s = |i: usize| HEP[i % 12];
        for (bi, (o, c)) in [("(", ")"), ("[", "]")].iter().enumerate() {
            let mi = 6;
            let m_ = "\u{60C}";
            let two = rep(2, |i| format!("{} {} <b>{o}</b>{}<b>{c}</b> {o}{}{c} {}{m_}", s(i + mi), s(i + mi + 1), s(i + mi + 2), s(i + mi + 3), s(i + mi + 4)));
            let tag = format!("f3/fvx-c-hep-m6-b{bi}-two-none");
            let m = measure(&tag, "he", "p { letter-spacing: 0.5em; text-align: justify; margin: 0 25% }", &format!("<p>{two}</p>"));
            r.push((tag, m.cons));
        }
    }
    check("h4_f3_f4_vs_b", &r);
}

// ---- H6 mixed runs: cons at M.
#[test]
fn h6_mixed_runs_vs_m() {
    let mut r = Vec::new();
    for st in ["", "p { word-spacing: 0.6em }"] {
        for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
            let s = |i: usize| w[i % 12];
            for (sn, end) in [("dotex", ".!"), ("exell", "!\u{2026}")] {
                let body = format!("<p dir=\"rtl\">{} {}<br/>{end}<b>(</b>{}<b>)</b> {} {}</p>", s(0), s(8), s(3), s(4), s(11));
                let tag = format!("h6/{sc}/{sn}-none/opens/rtl [{st}]");
                let m = measure(&tag, lang, st, &body);
                r.push((tag, m.cons));
            }
        }
    }
    check("h6_mixed_runs_vs_m", &r);
}

fn price_brb(w: &[&str; 12], mi: usize, m: &str, o: &str, c: &str) -> String {
    let s = |i: usize| w[i % 12];
    format!("<p dir=\"rtl\">{} {}<br/>{m}<b>{o}</b>{}<b>{c}</b> {} {}</p>", s(mi), s(mi + 1), s(mi + 2), s(mi + 3), s(mi + 4))
}

// ---- Interaction books: logical order at M (default 9, bundled 6).
#[test]
fn interaction_logical_vs_m() {
    let mut r = Vec::new();
    let brackets = [("(", ")"), ("[", "]"), ("{", "}")];
    // price marks index: qe = 6 ("?!"), eq = 8 ("!?").
    let mut set: Vec<(&str, &str, &[&str; 12], usize, &str, &str)> = vec![
        ("p { word-spacing: 0.6em; margin: 0 20% }", "ar", &AR, 8, "!?", "eq"),
        ("p { word-spacing: 1em; margin: 0 20% }", "ar", &AR, 8, "!?", "eq"),
        ("p { word-spacing: 1em; margin: 0 20% }", "he", &HE, 6, "?!", "qe"),
    ];
    for (st, lang, w, mi, m, mn) in set {
        for (bi, (o, c)) in brackets.iter().enumerate() {
            let tag = format!("int/price/{lang}/{mn}/b{bi}/brb/rtl [{st}]");
            let mm = measure(&tag, lang, st, &price_brb(w, mi, m, o, c));
            r.push((tag, mm.logical));
        }
    }
    check("interaction_logical_vs_m", &r);
}

// ---- `'.` books: logical order at B.
#[test]
fn apos_stop_logical_vs_b() {
    let mut r = Vec::new();
    let brackets = [("(", ")"), ("[", "]"), ("{", "}")];
    let st = "p { word-spacing: 0.5em; margin: 0 20% }";
    for (bi, (o, c)) in brackets.iter().enumerate() {
        let tag = format!("apos/price/ar/apos-stop/b{bi}/brb/rtl");
        let mm = measure(&tag, "ar", st, &price_brb(&AR, 13, "'.", o, c));
        r.push((tag, mm.logical));
    }
    check("apos_stop_logical_vs_b", &r);
}
