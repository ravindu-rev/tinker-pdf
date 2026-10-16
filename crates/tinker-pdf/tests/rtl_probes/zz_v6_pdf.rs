//! `zz_v6_pdf` (v6pdf, round 6 lane V (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_v6_pdf.rs`, which is the probe as it ran.
//!
//! Plain synthetic PDFs, not EPUB, whose right-to-left text ruling 14's
//! extraction reads: one logical line a case, drawn as a producer of PDFs
//! draws it (one string with and without `Tw`, a `TJ` array whose words are
//! parted by displacements, a word or a glyph a time in visual or in reading
//! order, letter-spaced, kerned).

use std::fmt::Write as _;
use tinker_pdf::Document;
use tinker_pdf_cos::build::DocumentBuilder;

/// (character, glyph name, width in thousandths)
fn glyphs() -> Vec<(char, String, u16)> {
    let mut out: Vec<(char, String, u16)> = Vec::new();
    let named: [(char, &str, u16); 26] = [
        (' ', "space", 250),
        ('!', "exclam", 300),
        ('"', "quotedbl", 350),
        ('%', "percent", 800),
        ('\'', "quotesingle", 200),
        ('(', "parenleft", 300),
        (')', "parenright", 300),
        (',', "comma", 250),
        ('-', "hyphen", 300),
        ('.', "period", 250),
        ('/', "slash", 300),
        (':', "colon", 250),
        (';', "semicolon", 250),
        ('?', "question", 500),
        ('0', "zero", 500),
        ('1', "one", 500),
        ('2', "two", 500),
        ('3', "three", 500),
        ('4', "four", 500),
        ('5', "five", 500),
        ('6', "six", 500),
        ('7', "seven", 500),
        ('8', "eight", 500),
        ('9', "nine", 500),
        ('\u{AB}', "guillemotleft", 500),
        ('\u{BB}', "guillemotright", 500),
    ];
    for (c, n, w) in named {
        out.push((c, n.to_owned(), w));
    }
    for c in 'a'..='z' {
        out.push((c, c.to_string(), 500));
    }
    for c in ['T', 'W'] {
        out.push((c, c.to_string(), 600));
    }
    for u in 0x5D0..=0x5EA {
        out.push((char::from_u32(u).unwrap(), format!("uni{u:04X}"), 550));
    }
    for u in [
        0x627, 0x628, 0x62A, 0x62C, 0x62F, 0x631, 0x633, 0x639, 0x642, 0x643, 0x644, 0x645, 0x646,
        0x647, 0x648, 0x64A, 0x629, 0x623, 0x637,
    ] {
        out.push((char::from_u32(u).unwrap(), format!("uni{u:04X}"), 520));
    }
    for u in 0x660..=0x669 {
        out.push((char::from_u32(u).unwrap(), format!("uni{u:04X}"), 500));
    }
    for u in [0x60C, 0x61F, 0x66A, 0x66B, 0x66C] {
        out.push((char::from_u32(u).unwrap(), format!("uni{u:04X}"), 300));
    }
    out
}

struct Font {
    table: Vec<(char, String, u16)>,
}

impl Font {
    fn code(&self, c: char) -> (u8, f64) {
        let at = self
            .table
            .iter()
            .position(|(ch, _, _)| *ch == c)
            .unwrap_or_else(|| panic!("no glyph for {c:?}"));
        (32 + at as u8, f64::from(self.table[at].2) / 1000.0)
    }
    fn string(&self, s: &str) -> String {
        let mut out = String::new();
        for c in s.chars() {
            let (code, _) = self.code(c);
            match code {
                b'(' | b')' | b'\\' => {
                    out.push('\\');
                    out.push(code as char)
                }
                33..=126 | 32 => out.push(code as char),
                _ => {
                    let _ = write!(out, "\\{code:03o}");
                }
            }
        }
        out
    }
    fn width(&self, s: &str, size: f64, tc: f64, tw: f64) -> f64 {
        s.chars()
            .map(|c| {
                let (code, w) = self.code(c);
                w * size + tc + if code == 32 { tw } else { 0.0 }
            })
            .sum()
    }
}

fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '\u{AB}' => '\u{BB}',
        '\u{BB}' => '\u{AB}',
        x => x,
    }
}

/// A line: its tokens in logical order, each right-to-left (`R`, drawn
/// reversed and mirrored) or left-to-right (`L`, kept), white space between.
struct Line {
    id: String,
    rtl_base: bool,
    tokens: Vec<(String, bool)>,
}

impl Line {
    fn logical(&self) -> String {
        self.tokens
            .iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
    /// The line in visual order, left to right, its white space kept, as a
    /// producer that ran UAX #9 draws it, split back into words at spaces.
    ///
    /// Levels, by token: in a right-to-left paragraph a right-to-left token
    /// is at 1 whole, a left-to-right one (Latin, any digits) at 2 but for the
    /// punctuation at its ends, at 1 (a European number keeps a trailing `%`);
    /// in a left-to-right paragraph a left-to-right token is at 0 whole and a
    /// right-to-left one at 1 but for the punctuation at its ends, at 0. A
    /// space between two tokens is at the higher level only where both of its
    /// neighbours are. Then L2: from the highest level down to 1, every run at
    /// that level or above reversed; and L4: a character at an odd level is
    /// mirrored.
    fn visual_string(&self) -> String {
        let (lo, hi) = if self.rtl_base {
            (1u8, 2u8)
        } else {
            (0u8, 1u8)
        };
        let mut chars: Vec<(char, u8)> = Vec::new();
        for (at, (t, r)) in self.tokens.iter().enumerate() {
            if at > 0 {
                chars.push((' ', lo));
            }
            let inner = if self.rtl_base { !*r } else { *r };
            let v: Vec<char> = t.chars().collect();
            let core = |c: char| c.is_alphanumeric();
            let first = v.iter().position(|c| core(*c)).unwrap_or(v.len());
            let mut last = v.iter().rposition(|c| core(*c)).map_or(0, |p| p + 1);
            if inner
                && self.rtl_base
                && v.get(last) == Some(&'%')
                && v[..last].iter().any(|c| c.is_ascii_digit())
            {
                last += 1;
            }
            for (k, c) in v.iter().enumerate() {
                let level = if inner && k >= first && k < last {
                    hi
                } else {
                    lo
                };
                chars.push((*c, level));
            }
        }
        // A space whose neighbours are both at the higher level is at it too.
        for k in 1..chars.len().saturating_sub(1) {
            if chars[k].0 == ' ' && chars[k - 1].1 == hi && chars[k + 1].1 == hi {
                chars[k].1 = hi;
            }
        }
        let max = chars.iter().map(|(_, l)| *l).max().unwrap_or(0);
        let mut level = max;
        while level >= 1 {
            let mut k = 0;
            while k < chars.len() {
                if chars[k].1 >= level {
                    let start = k;
                    while k < chars.len() && chars[k].1 >= level {
                        k += 1;
                    }
                    chars[start..k].reverse();
                } else {
                    k += 1;
                }
            }
            level -= 1;
        }
        chars
            .iter()
            .map(|(c, l)| if l % 2 == 1 { mirror(*c) } else { *c })
            .collect()
    }
    /// The visual order's words, left to right, each as drawn.
    fn visual(&self) -> Vec<String> {
        self.visual_string()
            .split(' ')
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect()
    }
}

fn r(s: &str) -> (String, bool) {
    (s.to_owned(), true)
}
fn l(s: &str) -> (String, bool) {
    (s.to_owned(), false)
}

fn lines() -> Vec<Line> {
    let mut out = Vec::new();
    let he: Vec<(&str, Vec<(String, bool)>)> = vec![
        ("plain", vec![r("שלום"), r("עולם"), r("גדול.")]),
        ("en-num", vec![r("בית"), l("2010"), r("גדול.")]),
        ("gloss", vec![r("ספר"), l("abc"), l("def"), r("בית.")]),
        ("brackets", vec![r("אור"), r("(יום)"), r("ספר.")]),
        ("sepnum", vec![r("עולם"), l("1,234.5"), r("שלום")]),
        ("qe", vec![r("שלום"), r("עולם?!")]),
        ("guil", vec![r("\u{AB}ספר\u{BB}"), r("בית.")]),
        ("pct", vec![l("50%"), r("שלום"), r("עולם.")]),
        ("num-stop", vec![r("בראשית."), r("אור"), l("60.")]),
        ("latin-first", vec![l("abc"), r("שלום"), r("עולם.")]),
        ("commas", vec![r("ספר,"), r("בית,"), r("אור.")]),
        ("colon", vec![r("שעה"), l("10:30"), r("בבוקר.")]),
        ("date", vec![r("יום"), l("16/10/2026"), r("שלום.")]),
        ("range", vec![r("עמודים"), l("3-7"), r("ספר.")]),
    ];
    let ar: Vec<(&str, Vec<(String, bool)>)> = vec![
        ("plain", vec![r("كتاب"), r("مدرسة"), r("جميل.")]),
        ("an-num", vec![r("بيت"), l("١٢٣"), r("كبير.")]),
        ("an-sep", vec![r("نور"), l("١٬٢٣٤٫٥"), r("يوم.")]),
        ("en-num", vec![r("بيت"), l("2010"), r("كبير.")]),
        ("gloss", vec![r("كتاب"), l("abc"), l("def"), r("بيت.")]),
        ("brackets", vec![r("نور"), r("(يوم)"), r("كتاب.")]),
        ("acomma", vec![r("كتاب،"), r("بيت،"), r("نور.")]),
        ("aq", vec![r("هل"), r("كتاب؟")]),
        ("apct", vec![r("نور"), l("٥٠٪"), r("يوم.")]),
        ("en-pct", vec![r("نور"), l("25%"), r("يوم.")]),
    ];
    for (n, t) in he {
        out.push(Line {
            id: format!("he/{n}"),
            rtl_base: true,
            tokens: t,
        });
    }
    for (n, t) in ar {
        out.push(Line {
            id: format!("ar/{n}"),
            rtl_base: true,
            tokens: t,
        });
    }
    // Left-to-right paragraphs quoting right-to-left words.
    out.push(Line {
        id: "ltr/he-phrase".into(),
        rtl_base: false,
        tokens: vec![
            l("The"),
            l("word"),
            r("שלום"),
            r("עולם"),
            l("means"),
            l("peace."),
        ],
    });
    out.push(Line {
        id: "ltr/he-bracket".into(),
        rtl_base: false,
        tokens: vec![l("abc"), r("(שלום)"), l("def.")],
    });
    out.push(Line {
        id: "ltr/ar-phrase".into(),
        rtl_base: false,
        tokens: vec![l("a"), r("كتاب"), r("جميل"), l("b.")],
    });
    out
}

/// One page's content: the line drawn as `pattern` says, at `size`, from the
/// right margin leftwards for a right-to-left paragraph.
fn content(font: &Font, line: &Line, pattern: &str, size: f64) -> String {
    let visual = line.visual();
    let joined = line.visual_string();
    let y = 700.0;
    let right = 500.0;
    let mut s = String::new();
    let start = |w: f64| if line.rtl_base { right - w } else { 72.0 };
    match pattern {
        "line-tj" | "line-tw" | "line-tc" => {
            let (tc, tw) = match pattern {
                "line-tw" => (0.0, 3.0),
                "line-tc" => (1.2, 0.0),
                _ => (0.0, 0.0),
            };
            let w = font.width(&joined, size, tc, tw);
            let _ = write!(
                s,
                "BT /F0 {size} Tf {tc} Tc {tw} Tw {} {y} Td ({}) Tj ET\n",
                start(w),
                font.string(&joined)
            );
        }
        "tj-gaps" | "tj-kern" => {
            // Words parted by a displacement and no space glyph, or kerned
            // inside and spaced with a glyph.
            let mut arr = String::new();
            let mut w = 0.0;
            for (at, tok) in visual.iter().enumerate() {
                if pattern == "tj-gaps" {
                    if at > 0 {
                        arr.push_str(" -300 ");
                        w += 0.3 * size;
                    }
                    let _ = write!(arr, "({})", font.string(tok));
                    w += font.width(tok, size, 0.0, 0.0);
                } else {
                    if at > 0 {
                        let _ = write!(arr, "({})", font.string(" "));
                        w += font.width(" ", size, 0.0, 0.0);
                    }
                    for (k, c) in tok.chars().enumerate() {
                        if k > 0 {
                            arr.push_str(" 30 ");
                            w -= 0.03 * size;
                        }
                        let cs = c.to_string();
                        let _ = write!(arr, "({})", font.string(&cs));
                        w += font.width(&cs, size, 0.0, 0.0);
                    }
                }
            }
            let _ = write!(
                s,
                "BT /F0 {size} Tf 0 Tc 0 Tw {} {y} Td [{arr}] TJ ET\n",
                start(w)
            );
        }
        "word-visual" | "word-reading" | "word-objects" => {
            // Each word at its own place, no space drawn, a space's width apart.
            let space = font.width(" ", size, 0.0, 0.0);
            let total: f64 = visual
                .iter()
                .map(|t| font.width(t, size, 0.0, 0.0))
                .sum::<f64>()
                + space * (visual.len() as f64 - 1.0);
            let mut x = start(total);
            let mut placed: Vec<(f64, &String)> = Vec::new();
            for tok in &visual {
                placed.push((x, tok));
                x += font.width(tok, size, 0.0, 0.0) + space;
            }
            if pattern == "word-reading" && line.rtl_base {
                placed.reverse();
            }
            if pattern == "word-objects" {
                for (x, tok) in placed {
                    let _ = write!(
                        s,
                        "BT /F0 {size} Tf {x:.3} {y} Td ({}) Tj ET\n",
                        font.string(tok)
                    );
                }
            } else {
                s.push_str(&format!("BT /F0 {size} Tf\n"));
                for (x, tok) in placed {
                    let _ = write!(s, "1 0 0 1 {x:.3} {y} Tm ({}) Tj\n", font.string(tok));
                }
                s.push_str("ET\n");
            }
        }
        "glyph-visual" | "glyph-reading" => {
            let w = font.width(&joined, size, 0.0, 0.0);
            let mut x = start(w);
            let mut placed: Vec<(f64, char)> = Vec::new();
            for c in joined.chars() {
                placed.push((x, c));
                x += font.width(&c.to_string(), size, 0.0, 0.0);
            }
            if pattern == "glyph-reading" && line.rtl_base {
                placed.reverse();
            }
            s.push_str(&format!("BT /F0 {size} Tf\n"));
            for (x, c) in placed {
                if c == ' ' {
                    continue;
                }
                let _ = write!(
                    s,
                    "1 0 0 1 {x:.3} {y} Tm ({}) Tj\n",
                    font.string(&c.to_string())
                );
            }
            s.push_str("ET\n");
        }
        _ => {}
    }
    s
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn esc(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c.to_string()
            } else if c == '\n' {
                "\\n".into()
            } else {
                format!("\\u{{{:x}}}", c as u32)
            }
        })
        .collect()
}

/// Every book of the family, as its probe built them, in order.
pub(crate) fn run() {
    let font = Font { table: glyphs() };
    assert!(font.table.len() <= 224, "{} glyphs", font.table.len());
    let names: Vec<&str> = font.table.iter().map(|(_, n, _)| n.as_str()).collect();
    let widths: Vec<u16> = font.table.iter().map(|(_, _, w)| *w).collect();
    let mut n = 0usize;
    for pattern in [
        "line-tj",
        "line-tw",
        "line-tc",
        "tj-gaps",
        "tj-kern",
        "word-visual",
        "word-reading",
        "word-objects",
        "glyph-visual",
        "glyph-reading",
    ] {
        for size in [10.0, 7.0] {
            for line in lines() {
                let mut builder = DocumentBuilder::new();
                assert!(builder.add_named_font(b"F0", b"Helvetica", 32, &names, &widths));
                let c = content(&font, &line, pattern, size);
                builder.add_page(612.0, 792.0, |page| page.raw(c.as_bytes()));
                let bytes = builder.finish();
                let doc = Document::open(bytes).expect("opens");
                let page = doc.page(0).expect("a page");
                let text = page.text().plain_text();
                let want = norm(&line.logical());
                let got = norm(&text);
                let words: Vec<String> = line
                    .logical()
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|w| w.chars().count() >= 2)
                    .map(str::to_owned)
                    .collect();
                let t = page.text();
                let found = words.iter().filter(|w| !t.search(w).is_empty()).count();
                let whole = (got.contains(&want)) as u8;
                let same = (got == want) as u8;
                emit!(
                    "ROW\t{pattern}@{size}\t{}\tm=0\ttext={whole}\tsearch={found}/{}\tltr=-\tcons={same}\tlogical={same}\tride=0\toverlap=0/0\t{}",
                    line.id,
                    words.len(),
                    esc(&text)
                );
                n += 1;
            }
        }
    }
    emit!("books={n}");
}
