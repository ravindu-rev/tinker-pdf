//! `zz_c1_tw` (tw, round-5 review (confirm)): the books of
//! `tools/probes/rtl/probes/pdf/zz_c1_tw.rs`, which is the probe as it ran.
//!
//! confirm1 probe: ROADMAP CD-19's first clause read
//! from the content streams — every string the standard-14 overflow font
//! (`/BxN`) shows that holds a code 32 is shown at a word spacing of zero —
//! and the second, no glyph drawn over another, over books built here (word
//! spacing, justification, letter-spacing, marks or none, styled runs).

use super::common::{esc, nsm};
use crate::epub_support::book::styled_book;
use crate::epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::{Document, TextOptions};

#[derive(Debug, Clone)]
enum Tok {
    Num(f64),
    Str(Vec<u8>),
    Name(Vec<u8>),
    ArrOpen,
    ArrClose,
    Op(Vec<u8>),
    Other,
}

fn lex(b: &[u8]) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0;
    let delim = |c: u8| {
        matches!(
            c,
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
        )
    };
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() || c == 0 {
            i += 1;
        } else if c == b'%' {
            while i < b.len() && b[i] != b'\n' && b[i] != b'\r' {
                i += 1;
            }
        } else if c == b'(' {
            let mut depth = 1;
            let mut s = Vec::new();
            i += 1;
            while i < b.len() && depth > 0 {
                let d = b[i];
                if d == b'\\' && i + 1 < b.len() {
                    let e = b[i + 1];
                    i += 2;
                    match e {
                        b'n' => s.push(b'\n'),
                        b'r' => s.push(b'\r'),
                        b't' => s.push(b'\t'),
                        b'b' => s.push(8),
                        b'f' => s.push(12),
                        b'0'..=b'7' => {
                            let mut v = u32::from(e - b'0');
                            let mut k = 0;
                            while k < 2 && i < b.len() && (b'0'..=b'7').contains(&b[i]) {
                                v = v * 8 + u32::from(b[i] - b'0');
                                i += 1;
                                k += 1;
                            }
                            s.push((v & 0xff) as u8);
                        }
                        b'\n' => {}
                        b'\r' => {
                            if i < b.len() && b[i] == b'\n' {
                                i += 1;
                            }
                        }
                        other => s.push(other),
                    }
                    continue;
                }
                if d == b'(' {
                    depth += 1;
                } else if d == b')' {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                s.push(d);
                i += 1;
            }
            out.push(Tok::Str(s));
        } else if (c == b'<' || c == b'>') && i + 1 < b.len() && b[i + 1] == c {
            // `<<` and `>>`: a dictionary's ends, nothing a string shows.
            out.push(Tok::Other);
            i += 2;
        } else if c == b'<' {
            let mut hex = Vec::new();
            i += 1;
            while i < b.len() && b[i] != b'>' {
                if b[i].is_ascii_hexdigit() {
                    hex.push(b[i]);
                }
                i += 1;
            }
            i += 1;
            if hex.len() % 2 == 1 {
                hex.push(b'0');
            }
            let s = hex
                .chunks(2)
                .map(|p| {
                    u8::from_str_radix(std::str::from_utf8(p).unwrap_or("00"), 16).unwrap_or(0)
                })
                .collect();
            out.push(Tok::Str(s));
        } else if c == b'[' {
            out.push(Tok::ArrOpen);
            i += 1;
        } else if c == b']' {
            out.push(Tok::ArrClose);
            i += 1;
        } else if c == b'/' {
            let mut n = Vec::new();
            i += 1;
            while i < b.len() && !b[i].is_ascii_whitespace() && !delim(b[i]) {
                n.push(b[i]);
                i += 1;
            }
            out.push(Tok::Name(n));
        } else {
            let mut w = Vec::new();
            while i < b.len() && !b[i].is_ascii_whitespace() && !delim(b[i]) {
                w.push(b[i]);
                i += 1;
            }
            if w.is_empty() {
                i += 1;
                out.push(Tok::Other);
                continue;
            }
            match std::str::from_utf8(&w)
                .ok()
                .and_then(|s| s.parse::<f64>().ok())
            {
                Some(v) if w[0] != b'T' => out.push(Tok::Num(v)),
                _ => out.push(Tok::Op(w)),
            }
        }
    }
    out
}

#[derive(Default, Debug)]
struct TwCheck {
    bx_strings: usize,
    bx32: usize,
    bad: usize,
    bad_at: String,
    // a word space shown by a simple non-overflow font at Tw 0 under word spacing: not checked
}

fn check(content: &[u8], c: &mut TwCheck) {
    let toks = lex(content);
    let mut tw = 0.0f64;
    let mut font: Vec<u8> = Vec::new();
    let mut stack: Vec<(f64, Vec<u8>)> = Vec::new();
    let mut ops: Vec<Tok> = Vec::new();
    let mut in_arr = false;
    let mut arr: Vec<Vec<u8>> = Vec::new();
    let show = |s: &[u8], tw: f64, font: &[u8], c: &mut TwCheck| {
        if font.starts_with(b"Bx") {
            c.bx_strings += 1;
            if s.contains(&32) {
                c.bx32 += 1;
                if tw != 0.0 {
                    c.bad += 1;
                    if c.bad_at.is_empty() {
                        c.bad_at = format!(
                            "{} Tw {} {:?}",
                            String::from_utf8_lossy(font),
                            tw,
                            esc(&String::from_utf8_lossy(s))
                        );
                    }
                }
            }
        }
    };
    for t in toks {
        match t {
            Tok::ArrOpen => {
                in_arr = true;
                arr.clear();
            }
            Tok::ArrClose => {
                in_arr = false;
            }
            Tok::Str(s) if in_arr => arr.push(s),
            Tok::Op(op) => {
                match op.as_slice() {
                    b"q" => stack.push((tw, font.clone())),
                    b"Q" => {
                        if let Some((a, f)) = stack.pop() {
                            tw = a;
                            font = f;
                        }
                    }
                    b"Tw" => {
                        if let Some(Tok::Num(v)) = ops.last() {
                            tw = *v;
                        }
                    }
                    b"Tf" => {
                        if let Some(Tok::Name(n)) =
                            ops.iter().rev().find(|t| matches!(t, Tok::Name(_)))
                        {
                            font = n.clone();
                        }
                    }
                    b"Tj" | b"'" => {
                        if let Some(Tok::Str(s)) = ops.last() {
                            show(s, tw, &font, c);
                        }
                    }
                    b"\"" => {
                        if let (Some(Tok::Num(aw)), Some(Tok::Str(s))) =
                            (ops.get(ops.len().wrapping_sub(3)), ops.last())
                        {
                            tw = *aw;
                            show(s, tw, &font, c);
                        }
                    }
                    b"TJ" => {
                        for s in &arr {
                            show(s, tw, &font, c);
                        }
                        arr.clear();
                    }
                    _ => {}
                }
                ops.clear();
            }
            other => ops.push(other),
        }
    }
}

fn run_book(style: &str, name: &str, lang: &str, body: &str) {
    let bytes = styled_book(lang, style, body);
    let doc = match Document::open(bytes.clone()) {
        Ok(doc) => doc,
        Err(e) => {
            emit!(
                "TWROW\t{style}\t{name}\tOPEN-ERR {}",
                esc(&format!("{e:?}"))
            );
            return;
        }
    };
    let mut c = TwCheck::default();
    {
        let cos = doc.cos();
        for page in tinker_pdf_cos::pages::collect(cos) {
            check(&tinker_pdf_cos::pages::content_bytes(cos, &page), &mut c);
        }
    }
    let mut vov = 0usize;
    let mut vov_at = String::new();
    for at in 0..doc.page_count() {
        let page = doc.page(at).expect("a page");
        let drawn = page.text_with(&TextOptions {
            content_order: true,
        });
        let mut boxes: Vec<(i64, f64, f64, f64, String)> = Vec::new();
        for line in drawn.lines() {
            for ch in &line.chars {
                let (x0, _, x1, _) = ch.quad.bounds();
                if ch.text.chars().next().is_some_and(nsm) {
                    continue;
                }
                if !ch.text.trim().is_empty() {
                    boxes.push((
                        (ch.origin.1 * 10.0).round() as i64,
                        x0,
                        x1,
                        ch.size,
                        ch.text.clone(),
                    ));
                }
            }
        }
        boxes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        for pair in boxes.windows(2) {
            let (p, n) = (&pair[0], &pair[1]);
            if p.0 == n.0 && n.1 < p.2 - 0.02 * n.3 {
                vov += 1;
                if vov_at.is_empty() {
                    vov_at = format!(
                        "{}@{:.2}..{:.2}/{}@{:.2}",
                        esc(&p.4),
                        p.1,
                        p.2,
                        esc(&n.4),
                        n.1
                    );
                }
            }
        }
    }
    let cons = conservation(&bytes, &doc).holds() as u8;
    let logical = conservation_in_logical_order(&bytes, &doc).holds() as u8;
    emit!(
        "TWROW\t{style}\t{name}\tbx={}\tbx32={}\ttwbad={}\tvov={vov}\tcons={cons}\tlogical={logical}\t{}\t{vov_at}",
        c.bx_strings, c.bx32, c.bad, esc(&c.bad_at)
    );
}

const AR: [&str; 14] = [
    "كتاب",
    "مدرسة",
    "الطالب",
    "جميل",
    "بيت",
    "كبير",
    "نور",
    "يوم",
    "عوامل",
    "وقد",
    "أصبحنا",
    "نعرف",
    "مدة",
    "معلم",
];

const ARP: [&str; 8] = [
    "كِتَابٌ",
    "مَدْرَسَةٌ",
    "الطَّالِبُ",
    "جَمِيلٌ",
    "مُدَّةَ",
    "مُعلِّم",
    "أَصْبَحْنَا",
    "نَعْرِفُ",
];

const HE: [&str; 10] = [
    "שלום",
    "עולם",
    "מלחמה",
    "ספר",
    "בית",
    "גדול",
    "אור",
    "יום",
    "בראשית",
    "ברא",
];

const HEP: [&str; 8] = ["שָׁלוֹם", "עוֹלָם", "מִלְחָמָה", "סֵפֶר", "בַּיִת", "גָּדוֹל", "אוֹר", "יוֹם"];

const EL: [&str; 8] = [
    "αλφα",
    "βητα",
    "γαμμα",
    "δελτα",
    "λόγος",
    "ἀρχῇ",
    "θεός",
    "κόσμος",
];

const RU: [&str; 8] = [
    "аб",
    "привет",
    "мир",
    "книга",
    "слово",
    "дом",
    "свет",
    "день",
];

const EN: [&str; 8] = [
    "the", "word", "file", "open", "close", "read", "once", "more",
];

fn styles() -> Vec<String> {
    let mut out = vec!["p { margin: 0 30% }".to_owned()];
    for ws in ["4px", "0.5em", "0.5001em", "0.6em", "1em"] {
        out.push(format!("p {{ word-spacing: {ws}; margin: 0 20% }}"));
    }
    out.push("p { text-align: justify; margin: 0 30% }".to_owned());
    out.push("p { text-align: justify; margin: 0 38% }".to_owned());
    for ls in ["0.25em", "0.45em", "0.55em"] {
        out.push(format!(
            "p {{ letter-spacing: {ls}; text-align: justify; margin: 0 25% }}"
        ));
    }
    out.push("p { letter-spacing: 0.2em; word-spacing: 0.4em; margin: 0 30% }".to_owned());
    out.push("body { font-family: monospace } p { word-spacing: 0.5em; text-align: justify; margin: 0 25% }".to_owned());
    out.push(
        "body { font-family: sans-serif } p { word-spacing: 0.5em; margin: 0 20% }".to_owned(),
    );
    out.push(
        "body { font-family: sans-serif } p { text-align: justify; margin: 0 32% }".to_owned(),
    );
    out
}

fn cases() -> Vec<(String, &'static str, String)> {
    let mut out = Vec::new();
    let sets: [(&str, &str, &[&str]); 7] = [
        ("ar", "ar", &AR),
        ("arp", "ar", &ARP),
        ("he", "he", &HE),
        ("hep", "he", &HEP),
        ("el", "el", &EL),
        ("ru", "ru", &RU),
        ("en", "en", &EN),
    ];
    for (sc, lang, w) in sets {
        let s = |i: usize| w[i % w.len()];
        let plain = (0..14).map(s).collect::<Vec<_>>().join(" ") + ".";
        let styled = (0..14)
            .map(|i| {
                if i % 3 == 1 {
                    format!("<b>{}</b>", s(i))
                } else {
                    s(i).to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
            + ".";
        let inside = (0..6)
            .map(|i| {
                let word = s(i);
                let k = word
                    .char_indices()
                    .nth(2)
                    .map(|(at, _)| at)
                    .unwrap_or(word.len());
                format!(
                    "{} <b>{}</b>{} {}",
                    s(i + 6),
                    &word[..k],
                    &word[k..],
                    s(i + 3)
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
            + ".";
        let glued = (0..5)
            .map(|i| {
                format!(
                    "{} <b>.</b>{} <i>?!</i> {} <b>(</b>{}<b>)</b> {};",
                    s(i),
                    s(i + 1),
                    s(i + 2),
                    s(i + 3),
                    s(i + 4)
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        let numbers = (0..4)
            .map(|i| {
                format!(
                    "{} ٥٪ <b>{}</b> {} (١٢٣), {} 5% {}",
                    s(i),
                    s(i + 1),
                    s(i + 2),
                    s(i + 3),
                    s(i + 4)
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        let latin = (0..4)
            .map(|i| {
                format!(
                    "x {} {} y. {} ({}) {}",
                    s(i),
                    s(i + 1),
                    s(i + 2),
                    EN[i],
                    s(i + 3)
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        let punct = (0..4)
            .map(|i| {
                format!(
                    "{}; {}, {}. {}! {}? {}:",
                    s(i),
                    s(i + 1),
                    s(i + 2),
                    s(i + 3),
                    s(i + 4),
                    s(i + 5)
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        for (pn, text) in [
            ("plain", plain),
            ("styled", styled),
            ("inside", inside),
            ("glued", glued),
            ("numbers", numbers),
            ("latin", latin),
            ("punct", punct),
        ] {
            out.push((
                format!("{sc}/{pn}/rtl"),
                lang,
                format!("<p dir=\"rtl\">{text}</p>"),
            ));
            out.push((format!("{sc}/{pn}/none"), lang, format!("<p>{text}</p>")));
        }
        let table = format!(
            "<table dir=\"rtl\" style=\"border-spacing: 0; word-spacing: 1em\"><tr><td>abc</td><td>{} {}</td></tr><tr><td>def</td><td>{} {}</td></tr></table>",
            s(0), s(1), s(2), s(3)
        );
        out.push((format!("{sc}/table/rtl"), lang, table));
        let items = (0..3)
            .map(|i| format!("<li>{} {} {}</li>", s(i), s(i + 1), s(i + 2)))
            .collect::<String>();
        for (ln, ls) in [
            ("arabic-indic", "list-style-type: arabic-indic"),
            ("hebrew", "list-style-type: hebrew"),
            (
                "greek",
                "list-style-type: lower-greek; list-style-position: inside",
            ),
            (
                "inside",
                "list-style-type: arabic-indic; list-style-position: inside",
            ),
        ] {
            out.push((
                format!("{sc}/list-{ln}/rtl"),
                lang,
                format!("<ol dir=\"rtl\" style=\"{ls}\">{items}</ol>"),
            ));
            out.push((
                format!("{sc}/list-{ln}/none"),
                lang,
                format!("<ol style=\"{ls}\">{items}</ol>"),
            ));
        }
        out.push((
            format!("{sc}/heading/none"),
            lang,
            format!("<h2 style=\"text-align: justify; word-spacing: 0.5em\">{} {} {} {} {} {}</h2><p>{}</p>", s(0), s(1), s(2), s(3), s(4), s(5), s(6)),
        ));
        out.push((
            format!("{sc}/span-ws/none"),
            lang,
            format!(
                "<p>{} <span style=\"word-spacing: 1em\">{} {} {}</span> {} {}.</p>",
                s(0),
                s(1),
                s(2),
                s(3),
                s(4),
                s(5)
            ),
        ));
    }
    out.push((
        "doc/greek".to_owned(),
        "en",
        "<p>x <b>αλ</b>φα аб y.</p>".to_owned(),
    ));
    out.push((
        "doc/mudda".to_owned(),
        "ar",
        "<p dir=\"rtl\">مدة معلم.</p>".to_owned(),
    ));
    out.push((
        "doc/h4".to_owned(),
        "ar",
        "<p dir=\"rtl\">ا <b>.</b>أصبحنا ن</p>".to_owned(),
    ));
    out.push(("doc/pointed".to_owned(), "ar", "<p dir=\"rtl\">مُدَّةَ مُعلِّم. <b>مدة معلم.</b> مُدَّةَ مُعلِّم. <b>مدة معلم.</b> مُدَّةَ مُعلِّم. <b>مدة معلم.</b></p>".to_owned()));
    out.push((
        "doc/v3".to_owned(),
        "ar",
        "<p>مدرسة الطالب جميل بيت ٥٪ <b>كبير</b> نور يوم عوامل وقد أصبحنا ٥٪ <b>نعرف</b> كتاب</p>"
            .to_owned(),
    ));
    out
}

fn probe() {
    let mut n = 0usize;
    let cases = cases();
    for style in styles() {
        for (name, lang, body) in &cases {
            n += 1;
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_book(&style, name, lang, body)
            }));
            if r.is_err() {
                emit!("TWROW\t{style}\t{name}\tPANIC");
            }
        }
    }
    emit!("twbooks={n}");
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
