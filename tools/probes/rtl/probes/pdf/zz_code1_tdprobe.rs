//! Review probe (round 5, code1): hand-built content streams through
//! `TextDevice` and ruling 14, to list every difference between two trees.
//! Writes one TSV row per case to `$ZZ_OUT`: id, stream-order lines (with
//! rtl flags), logical plain text.
use std::fmt::Write as _;
use tinker_pdf::{Document, TextOptions};
use tinker_pdf_cos::build::DocumentBuilder;

const SIZE: f64 = 10.0;

struct Font {
    res: &'static [u8],
    first: u8,
    chars: &'static [(char, &'static str, u16)],
}

const W0: Font = Font {
    res: b"W0",
    first: 32,
    chars: &[
        (' ', "space", 250),
        ('a', "a", 500),
        ('b', "b", 500),
        ('c', "c", 500),
        ('d', "d", 500),
        ('e', "e", 500),
        ('f', "f", 500),
        ('.', "period", 250),
        (',', "comma", 250),
        ('(', "parenleft", 300),
        (')', "parenright", 300),
        ('!', "exclam", 300),
        ('?', "question", 500),
        ('"', "quotedbl", 350),
        ('\u{2026}', "ellipsis", 1000),
        ('\u{BB}', "guillemotright", 500),
        ('[', "bracketleft", 300),
        (']', "bracketright", 300),
        ('\u{201D}', "quotedblright", 350),
        (':', "colon", 250),
        ('1', "one", 500),
        ('2', "two", 500),
        ('{', "braceleft", 300),
        ('}', "braceright", 300),
        ('\'', "quotesingle", 200),
        ('\u{2019}', "quoteright", 200),
        ('\u{60C}', "uni060C", 250),
    ],
};

// Hebrew, ALEF at code 32 (the overflow font's shape).
const H0: Font = Font {
    res: b"H0",
    first: 32,
    chars: &[
        ('\u{5D0}', "uni05D0", 500),
        ('\u{5D1}', "uni05D1", 500),
        ('\u{5D2}', "uni05D2", 500),
        ('\u{5D3}', "uni05D3", 500),
        ('\u{5D4}', "uni05D4", 500),
        ('\u{5D5}', "uni05D5", 500),
        ('\u{5D9}', "uni05D9", 500),
        ('\u{5DD}', "uni05DD", 500),
    ],
};

// Hebrew from code 33: no letter at code 32.
const H1: Font = Font {
    res: b"H1",
    first: 33,
    chars: &[
        ('\u{5D0}', "uni05D0", 500),
        ('\u{5D1}', "uni05D1", 500),
        ('\u{5D2}', "uni05D2", 500),
        ('\u{5D3}', "uni05D3", 500),
        ('\u{5D4}', "uni05D4", 500),
        ('\u{5D5}', "uni05D5", 500),
        ('\u{5D9}', "uni05D9", 500),
        ('\u{5DD}', "uni05DD", 500),
    ],
};

// Arabic, ALEF at code 32.
const A0: Font = Font {
    res: b"A0",
    first: 32,
    chars: &[
        ('\u{627}', "uni0627", 300),
        ('\u{628}', "uni0628", 500),
        ('\u{62A}', "uni062A", 500),
        ('\u{646}', "uni0646", 500),
        ('\u{639}', "uni0639", 500),
        ('\u{631}', "uni0631", 400),
        ('\u{648}', "uni0648", 400),
        ('\u{64A}', "uni064A", 500),
        ('\u{645}', "uni0645", 500),
        ('\u{661}', "uni0661", 500),
        ('\u{66A}', "uni066A", 500),
        ('\u{60C}', "uni060C", 250),
    ],
};

const FONTS: [&Font; 4] = [&W0, &H0, &H1, &A0];

fn font_of(res: &[u8]) -> &'static Font {
    FONTS.iter().copied().find(|f| f.res == res).expect("font")
}

fn code(font: &Font, c: char) -> Option<(u8, u16)> {
    font.chars
        .iter()
        .position(|(ch, _, _)| *ch == c)
        .map(|at| (font.first + at as u8, font.chars[at].2))
}

/// One text object: font, Tc, Tw (points), text; placed at `x`.
#[derive(Clone)]
struct Obj {
    font: &'static [u8],
    tc: f64,
    tw: f64,
    text: String,
    tz: f64,
    rot: bool,
}

/// Where a PDF interpreter leaves the pen after `obj` drawn from `x`
/// (9.4.4, `Tw` after every single-byte code 32), and where its last box
/// ends.
fn advance(obj: &Obj, x: f64) -> (f64, f64) {
    let font = font_of(obj.font);
    let mut pen = x;
    let mut box_end = x;
    for c in obj.text.chars() {
        let (code, w) = code(font, c).expect("covered");
        let th = obj.tz / 100.0;
        box_end = pen + f64::from(w) / 1000.0 * SIZE * th;
        pen = box_end + (obj.tc + if code == 32 { obj.tw } else { 0.0 }) * th;
    }
    (pen, box_end)
}

fn stream(objs: &[(Obj, f64)]) -> Vec<u8> {
    let mut out = String::new();
    for (obj, x) in objs {
        let font = font_of(obj.font);
        let mut bytes = String::new();
        for c in obj.text.chars() {
            let (code, _) = code(font, c).expect("covered");
            match code {
                b'(' | b')' | b'\\' => {
                    bytes.push('\\');
                    bytes.push(code as char)
                }
                32..=126 => bytes.push(code as char),
                _ => {
                    let _ = write!(bytes, "\\{code:03o}");
                }
            }
        }
        let place = if obj.rot {
            format!("0 1 -1 0 400 {x} Tm")
        } else {
            format!("{x} 700 Td")
        };
        let _ = writeln!(
            out,
            "BT /{} {SIZE} Tf {} Tc {} Tw {} Tz {place} ({bytes}) Tj ET",
            String::from_utf8_lossy(obj.font),
            obj.tc,
            obj.tw,
            obj.tz
        );
    }
    out.into_bytes()
}

fn pdf(objs: &[(Obj, f64)]) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    for font in FONTS {
        let names: Vec<&str> = font.chars.iter().map(|(_, n, _)| *n).collect();
        let widths: Vec<u16> = font.chars.iter().map(|(_, _, w)| *w).collect();
        assert!(builder.add_named_font(font.res, b"Helvetica", font.first, &names, &widths));
    }
    let content = stream(objs);
    builder.add_page(900.0, 800.0, |page| page.raw(&content));
    builder.finish()
}

fn read(bytes: Vec<u8>) -> (String, String) {
    let doc = Document::open(bytes).expect("opens");
    let page = doc.page(0).expect("page");
    let stream = page.text_with(&TextOptions {
        content_order: true,
        ..TextOptions::default()
    });
    let lines: Vec<String> = stream
        .lines()
        .iter()
        .map(|l| format!("[{}{}]", if l.rtl { "R:" } else { "" }, l.text))
        .collect();
    let logical = page.text().plain_text();
    (lines.join(""), logical.replace('\n', "\\n"))
}

/// A row of cells: each cell a list of objects drawn end to end where the
/// interpreter leaves the pen; between cells, `gap` past the interpreter's
/// pen (`from_box`: past the last box's end instead).
fn row(cells: &[Vec<Obj>], gap: f64, from_box: bool) -> Vec<(Obj, f64)> {
    let mut out = Vec::new();
    let mut pen = 50.0;
    let mut last_box = 50.0;
    for (at, cell) in cells.iter().enumerate() {
        if at > 0 {
            pen = if from_box { last_box + gap } else { pen + gap };
        }
        for obj in cell {
            out.push((obj.clone(), pen));
            let (p, b) = advance(obj, pen);
            pen = p;
            last_box = b;
        }
    }
    out
}

fn o(font: &'static [u8], tc: f64, tw: f64, text: &str) -> Obj {
    Obj {
        font,
        tc,
        tw,
        text: text.to_owned(),
        tz: 100.0,
        rot: false,
    }
}

/// A cell of right-to-left words drawn left to right (visual order), each
/// word a text object, a `W0` space between them as its own object.
fn rtl_cell(font: &'static [u8], tc: f64, tw: f64, words: &[&str]) -> Vec<Obj> {
    let mut out = Vec::new();
    for (at, w) in words.iter().enumerate() {
        if at > 0 {
            out.push(o(b"W0", tc, tw, " "));
        }
        out.push(o(font, tc, tw, w));
    }
    out
}

#[test]
fn zz_code1_tdprobe() {
    let path = std::env::var("ZZ_OUT").unwrap_or_else(|_| "/dev/null".into());
    let mut out = String::new();
    let mut n = 0usize;
    let mut emit = |id: String, objs: Vec<(Obj, f64)>| {
        let (lines, logical) = read(pdf(&objs));
        let _ = writeln!(out, "{id}\t{lines}\t{logical}");
        n += 1;
    };

    // Family A: a first cell, a gap, then a right-to-left cell of two words
    // whose word space is stretched; and a third cell.
    let firsts: [(&str, Vec<&str>); 8] = [
        ("ltr", vec!["abc"]),
        ("ltrdot", vec!["abc."]),
        ("ltrsp", vec!["abc "]),
        ("digits", vec!["12"]),
        ("bracket", vec!["["]),
        ("rtl", vec![]),
        ("mixed", vec!["ab"]),
        ("quote", vec!["\""]),
    ];
    let hebrew = ["\u{5D3}\u{5D2}", "\u{5D1}\u{5D0}"];
    let hebrew_a = ["\u{5D0}\u{5D1}", "\u{5D2}\u{5D3}"]; // alef first: code 32 first
    let arabic = ["\u{646}\u{639}", "\u{627}\u{628}"];
    let gaps: [(f64, bool); 13] = [
        (0.0, false),
        (0.04, false),
        (-0.04, false),
        (0.06, false),
        (0.2, false),
        (0.45, false),
        (0.55, false),
        (0.8, false),
        (1.5, false),
        (4.7, false),
        (0.55, true),
        (0.8, true),
        (2.9, true),
    ];
    for (fname, words) in [
        ("H0", &hebrew[..]),
        ("H0a", &hebrew_a[..]),
        ("H1", &hebrew[..]),
        ("A0", &arabic[..]),
    ] {
        let font: &'static [u8] = match fname {
            "A0" => b"A0",
            "H1" => b"H1",
            _ => b"H0",
        };
        for tc_em in [0.0, 0.1, 0.5, 0.6] {
            for tw_em in [0.0, 0.5, 1.0] {
                let tc = tc_em * SIZE;
                let tw = tw_em * SIZE;
                for (first_name, first_words) in &firsts {
                    let first: Vec<Obj> = if *first_name == "rtl" {
                        vec![o(font, tc, tw, words[0])]
                    } else if *first_name == "mixed" {
                        vec![o(b"W0", tc, tw, first_words[0]), o(font, tc, tw, words[1])]
                    } else {
                        vec![o(b"W0", tc, tw, first_words[0])]
                    };
                    let second = rtl_cell(font, tc, tw, words);
                    for (gap_em, from_box) in gaps {
                        let gap = gap_em * SIZE;
                        let id = format!(
                            "A/{fname}/tc{tc_em}/tw{tw_em}/{first_name}/g{gap_em}{}",
                            if from_box { "b" } else { "" }
                        );
                        emit(id.clone(), row(&[first.clone(), second.clone()], gap, from_box));
                        // Three cells: the RTL cell then a Latin one.
                        let third = vec![o(b"W0", tc, tw, "def")];
                        emit(
                            format!("{id}/3"),
                            row(&[first.clone(), second.clone(), third], gap, from_box),
                        );
                    }
                }
            }
        }
    }

    // Family B: ruling 14's sentence end. Visual lines: [end-marks]
    // [words and a pair] drawn left to right, as a dir-less paragraph's
    // second line draws `موي )رون(.`, plus right-to-left openings.
    let ends = [
        "", ".", "..", "...", "?!", "!!", "!?", "??", ".\"", ".\u{BB}", ".\u{201D}", "\u{2026}",
        "\"", "\u{201D}", "]", "\u{201D}.", ",", ":", "\u{60C}", "'.", ".'", "\"...", "\u{201D}\u{2026}",
        ".\u{2019}", "\u{2019}", "!", "?", "\u{2026}.", "\"\"", ".\"\"",
    ];
    for (lang, w1, w2, w3) in [
        ("he", "\u{5D5}\u{5D9}", "\u{5D1}\u{5D9}\u{5D3}", "\u{5D0}\u{5D5}"),
        ("ar", "\u{645}\u{648}\u{64A}", "\u{631}\u{648}\u{646}", "\u{628}\u{646}"),
    ] {
        let font: &'static [u8] = if lang == "he" { b"H1" } else { b"A0" };
        for (bname, open, close) in [("paren", "(", ")"), ("brack", "[", "]"), ("brace", "{", "}"), ("none", "", "")] {
            for end in ends {
                for side in ["right", "left"] {
                    for pairs in [1usize, 2] {
                        for tw_em in [0.0, 0.6] {
                            let tw = tw_em * SIZE;
                            // Drawn left to right: w1 ' ' close w2 open [' ' w3 ...] end
                            // ("right": the end at the right; "left": at the left).
                            let mut objs: Vec<Obj> = Vec::new();
                            if side == "left" && !end.is_empty() {
                                objs.push(o(b"W0", 0.0, tw, end));
                            }
                            objs.push(o(font, 0.0, tw, w1));
                            objs.push(o(b"W0", 0.0, tw, &format!(" {close}")));
                            objs.push(o(font, 0.0, tw, w2));
                            if !open.is_empty() {
                                objs.push(o(b"W0", 0.0, tw, open));
                            }
                            if pairs == 2 {
                                objs.push(o(b"W0", 0.0, tw, &format!(" {close}")));
                                objs.push(o(font, 0.0, tw, w3));
                                if !open.is_empty() {
                                    objs.push(o(b"W0", 0.0, tw, open));
                                }
                            }
                            if side == "right" && !end.is_empty() {
                                objs.push(o(b"W0", 0.0, tw, end));
                            }
                            let id = format!("B/{lang}/{bname}/{end}/{side}/p{pairs}/tw{tw_em}");
                            emit(id, row(&[objs], 0.0, false));
                        }
                    }
                }
            }
        }
    }

    // Family C: the code-32 letter under Tw in one string, and a styled
    // neutral glued after it (H4's shape), stream order and positions as an
    // interpreter puts them.
    for tw_em in [0.0, 0.5, 0.6, 1.0] {
        let tw = tw_em * SIZE;
        for (name, objs) in [
            ("h0word", vec![o(b"H0", 0.0, tw, "\u{5D3}\u{5D0}\u{5D1}"), o(b"W0", 0.0, tw, ".")]),
            ("h0first", vec![o(b"H0", 0.0, tw, "\u{5D0}\u{5D1}\u{5D2}"), o(b"W0", 0.0, tw, " "), o(b"H0", 0.0, tw, "\u{5D3}\u{5D4}")]),
            ("a0", vec![o(b"A0", 0.0, tw, "\u{646}\u{627}\u{628}"), o(b"W0", 0.0, tw, "."), o(b"W0", 0.0, tw, " "), o(b"A0", 0.0, tw, "\u{639}")]),
        ] {
            for gap_em in [0.0, 0.3, 0.6] {
                let id = format!("C/{name}/tw{tw_em}/g{gap_em}");
                let cells: Vec<Vec<Obj>> = objs.iter().map(|ob| vec![ob.clone()]).collect();
                emit(id, row(&cells, gap_em * SIZE, false));
            }
        }
    }

    // Family D: negative Tc, horizontal scaling, rotated text, at the cell
    // shape of family A (a Latin cell, a gap, a right-to-left cell of two
    // words with a stretched word space).
    for tz in [50.0, 100.0, 200.0] {
        for rot in [false, true] {
            for tc_em in [-0.05, 0.1, 0.6] {
                for tw_em in [0.0, 1.0] {
                    let (tc, tw) = (tc_em * SIZE, tw_em * SIZE);
                    let set = |mut ob: Obj| {
                        ob.tz = tz;
                        ob.rot = rot;
                        ob
                    };
                    let first = vec![set(o(b"W0", tc, tw, "abc"))];
                    let second: Vec<Obj> = rtl_cell(b"H1", tc, tw, &hebrew).into_iter().map(set).collect();
                    for gap_em in [0.0, 0.3, 0.6, 1.0] {
                        let id = format!("D/tz{tz}/rot{rot}/tc{tc_em}/tw{tw_em}/g{gap_em}");
                        emit(id, row(&[first.clone(), second.clone()], gap_em * SIZE * tz / 100.0, false));
                    }
                }
            }
        }
    }

    std::fs::write(&path, &out).expect("write");
    eprintln!("ZZ rows {n}");
}
