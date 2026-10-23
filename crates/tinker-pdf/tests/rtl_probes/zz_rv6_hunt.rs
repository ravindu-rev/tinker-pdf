//! `zz_rv6_hunt` (rv6, round 6 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rv6_hunt.rs`, which is the probe as it ran.

use super::common::{hash, rep, run_book, Case, AR, HE};
use crate::epub_support::book::styled_book;
use tinker_pdf::Document;

/// Glyph codes drawn inside `/Artifact` marked content (text shadows) and
/// outside it (the text), each in content-stream order.
fn shadow_codes(content: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let (mut art, mut text) = (Vec::new(), Vec::new());
    let mut stack: Vec<bool> = Vec::new();
    let mut last_name = String::new();
    let mut i = 0;
    let n = content.len();
    while i < n {
        let b = content[i];
        let inside = stack.iter().any(|a| *a);
        if b == b'(' {
            let mut depth = 1;
            i += 1;
            let mut s = Vec::new();
            while i < n && depth > 0 {
                let c = content[i];
                if c == b'\\' && i + 1 < n {
                    let e = content[i + 1];
                    if (b'0'..=b'7').contains(&e) {
                        let mut v = 0u32;
                        let mut k = 0;
                        while k < 3 && i + 1 < n && (b'0'..=b'7').contains(&content[i + 1]) {
                            v = v * 8 + u32::from(content[i + 1] - b'0');
                            i += 1;
                            k += 1;
                        }
                        s.push(v as u8);
                        i += 1;
                        continue;
                    }
                    s.push(match e {
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        b'b' => 8,
                        b'f' => 12,
                        x => x,
                    });
                    i += 2;
                    continue;
                }
                if c == b'(' {
                    depth += 1;
                }
                if c == b')' {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                s.push(c);
                i += 1;
            }
            if inside {
                art.extend(s)
            } else {
                text.extend(s)
            }
            continue;
        }
        if b == b'<' && i + 1 < n && content[i + 1] != b'<' {
            i += 1;
            let mut hex = Vec::new();
            while i < n && content[i] != b'>' {
                if content[i].is_ascii_hexdigit() {
                    hex.push(content[i]);
                }
                i += 1;
            }
            i += 1;
            if hex.len() % 2 == 1 {
                hex.push(b'0');
            }
            let s: Vec<u8> = hex
                .chunks(2)
                .map(|p| {
                    u8::from_str_radix(std::str::from_utf8(p).unwrap_or("00"), 16).unwrap_or(0)
                })
                .collect();
            if inside {
                art.extend(s)
            } else {
                text.extend(s)
            }
            continue;
        }
        if b == b'<' {
            i += 2;
            continue;
        }
        if b == b'/' {
            let s = i;
            i += 1;
            while i < n && !b" \t\r\n/<>[]()".contains(&content[i]) {
                i += 1;
            }
            last_name = String::from_utf8_lossy(&content[s..i]).into_owned();
            continue;
        }
        if b.is_ascii_alphabetic() {
            let s = i;
            while i < n
                && (content[i].is_ascii_alphanumeric()
                    || content[i] == b'*'
                    || content[i] == b'\''
                    || content[i] == b'"')
            {
                i += 1;
            }
            match &content[s..i] {
                b"BMC" | b"BDC" => stack.push(last_name == "/Artifact"),
                b"EMC" => {
                    stack.pop();
                }
                _ => {}
            }
            continue;
        }
        i += 1;
    }
    (art, text)
}

fn shadow_row(style: &str, name: &str, bytes: Vec<u8>) {
    let Ok(doc) = Document::open(bytes) else {
        return;
    };
    let cos = doc.cos();
    let mut all = Vec::new();
    for page in tinker_pdf_cos::pages::collect(cos) {
        all.extend_from_slice(&tinker_pdf_cos::pages::content_bytes(cos, &page));
    }
    let (art, text) = shadow_codes(&all);
    let bt = all
        .split(|b| b.is_ascii_whitespace())
        .filter(|t| *t == b"BT")
        .count();
    emit!(
        "SHADOW\t{style}\t{name}\tsame={}\tart={}\ttext={}\tbt={bt}\tah={:016x}\tth={:016x}",
        (art == text) as u8,
        art.len(),
        text.len(),
        hash(&art),
        hash(&text)
    );
}

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
            if style.contains("text-shadow") {
                let bytes = styled_book(lang, &style, &body);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    shadow_row(&style, &name, bytes)
                }));
            }
        }
    }
    emit!("books={n}");
}

/// Every book of the family, as its probe built them, in order.
pub(crate) fn run() {
    probe();
}

// Family rv6_hunt (round 6's independent review of ROADMAP CD-19's fix, its
// first hunt): the shapes the fix's read-back decides, each in a Hebrew and an
// Arabic paragraph, right-to-left and with no dir: a styled `?!` and styled
// brackets (`punct`), numbers with a NO-BREAK SPACE, a NARROW NO-BREAK SPACE, a
// FIGURE SPACE and Arabic separators beside a styled `?!` or `!?` (`nbsp`,
// `nnbsp`, `sep`), SOFT HYPHENs (`shy`), directional marks around styled runs
// (`fmt`), and two inline blocks in a line, one widened by scaleX(2) holding a
// word and a styled `?!`, one narrowed by scaleX(0.7) (`mixib`); a line
// repeated under transforms of none and scaleX(1) and over page breaks; and a
// Latin paragraph. Under no transform, a scale of one axis and a quarter turn
// on the paragraph, word spacing from 0.35em to 0.6em and letter spacing, each
// with and without a text shadow, whose books print a SHADOW line besides
// their ROW: whether the glyph codes drawn as the shadow are the text's.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<(&str, String)> = vec![
            (
                "punct",
                rep(3, |i| {
                    format!(
                        "{} <b>?!</b> {} {}. <i>(</i>{}<i>)</i> {}!",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "nbsp",
                rep(2, |i| {
                    format!(
                        "{} 1\u{A0}234 <i>?!</i> {} 2\u{A0}500,75 {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
            (
                "nnbsp",
                rep(2, |i| {
                    format!(
                        "{} 1\u{202F}234 <i>?!</i> {} 12\u{2007}%, {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
            (
                "sep",
                rep(2, |i| {
                    format!("{} <b>1\u{A0}234.5</b> {} \u{661}\u{A0}\u{662}\u{663}\u{664}\u{66B}\u{665} <i>!?</i> {}.", s(i), s(i + 1), s(i + 2))
                }),
            ),
            (
                "shy",
                rep(2, |i| {
                    format!(
                        "{}\u{AD}{} <b>{}</b> {}\u{AD}{}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "fmt",
                rep(2, |i| {
                    format!("{} \u{200F}<b>?!</b>\u{200F} {} <span>\u{200E}</span>{} <i>\u{200F}</i>{}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
            ),
            (
                "mixib",
                rep(2, |i| {
                    format!("{} <span style=\"display: inline-block; transform: scaleX(2)\">{} <b>?!</b></span> {} <span style=\"display: inline-block; transform: scaleX(0.7)\">{}</span> {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4))
                }),
            ),
        ];
        for (name, text) in &shapes {
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((
                    format!("{sc}/{name}/{d}"),
                    lang,
                    format!("<div class=\"w\"><p{attr}>{text}</p></div>"),
                ));
            }
        }
        let line = format!("{} <b>?!</b> {} 1\u{A0}234 {}.", s(0), s(1), s(2));
        out.push((format!("{sc}/repeat-xform/rtl"), lang, format!("<div class=\"w\"><p dir=\"rtl\">{line}</p><p dir=\"rtl\" style=\"transform: none\">{line}</p><p dir=\"rtl\" style=\"transform: scaleX(1)\">{line}</p></div>")));
        out.push((format!("{sc}/repeat-pages/rtl"), lang, format!("<div class=\"w\"><p dir=\"rtl\">{line}</p><p dir=\"rtl\" style=\"page-break-before: always\">{line}</p><p dir=\"rtl\" style=\"page-break-before: always\">{line} {line}</p></div>")));
    }
    let lat = rep(3, |k| {
        format!("The <b>quick</b> brown fox{k}, (jumps) over 25% \u{14B}a\u{151} of the <i>lazy</i> dog\u{2019}s back.")
    });
    out.push((
        "lat/pure/none".to_owned(),
        "en",
        format!("<div class=\"w\"><p>{lat}</p></div>"),
    ));
    out
}

fn styles() -> Vec<String> {
    let transforms = [
        "none",
        "scaleX(2)",
        "scaleX(0.5)",
        "scaleX(1.4)",
        "rotate(90deg)",
    ];
    let spacing = [
        "word-spacing: 0.35em",
        "word-spacing: 0.4em",
        "word-spacing: 0.5em",
        "word-spacing: 0.6em",
        "letter-spacing: 0.33em",
    ];
    let mut out = Vec::new();
    for p in transforms {
        for s in spacing {
            for shadow in ["", " text-shadow: 1px 1px #f00;"] {
                out.push(format!(
                    "p {{ margin: 0 30%; transform: {p}; {s};{shadow} }} .w {{ transform: none }}"
                ));
            }
        }
    }
    out
}
