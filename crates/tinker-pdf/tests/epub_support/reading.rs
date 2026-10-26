//! What a reader gets back from one book, and the cases that hold a book to
//! it. Each case is a [`Case`] in the test file of its mechanism: a style, a
//! body, and what the book reads. A case's reading is five verdicts:
//!
//! - `cons`: the pages conserve the source's text in content order
//!   ([`conservation`]);
//! - `logical`: and in the structure tree's order
//!   ([`conservation_in_logical_order`]);
//! - `whole`: every block's text, white space aside, is a run of the pages'
//!   plain text;
//! - `found`: every word of two letters or more is found by search;
//! - `apart`: no glyph is drawn over another on its baseline.

use super::book::styled_book;
use super::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::{Document, TextOptions};
use tinker_pdf_shape::unicode::{bidi_class, BidiClass};

/// A non-spacing mark, which rides on the letter before it.
pub fn nsm(c: char) -> bool {
    bidi_class(c) == BidiClass::NSM
}

/// A format character, which a reading may drop or keep.
pub fn format_char(c: char) -> bool {
    matches!(c, '\u{AD}' | '\u{61C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

/// Text with its format characters dropped and its white space one space.
pub fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| !format_char(*c))
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The elements that end a block of text.
pub const BLOCKS: [&str; 26] = [
    "p",
    "li",
    "td",
    "th",
    "tr",
    "figcaption",
    "div",
    "br",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "dt",
    "dd",
    "caption",
    "pre",
    "blockquote",
    "figure",
    "table",
    "ul",
    "ol",
    "dl",
    "section",
    "tbody",
];

/// The book's visible text, one string per block (and per preserved line).
pub fn segments(body: &str) -> Vec<String> {
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
            for d in chars.by_ref() {
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
        } else if c == '&' {
            let mut ent = String::new();
            for d in chars.by_ref() {
                if d == ';' {
                    break;
                }
                ent.push(d);
            }
            let ch = match ent.as_str() {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "nbsp" => Some('\u{A0}'),
                e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16)
                    .ok()
                    .and_then(char::from_u32),
                e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            };
            if let Some(ch) = ch {
                cur.push(ch);
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

/// The words of the blocks a search should find: each run of letters,
/// digits and marks with two letters or digits or more, once.
pub fn tokens(segs: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in segs {
        for t in s.split(|c: char| !(c.is_alphanumeric() || nsm(c))) {
            if t.chars()
                .filter(|c| c.is_alphanumeric() && !nsm(*c))
                .count()
                >= 2
                && !out.iter().any(|o| o == t)
            {
                out.push(t.to_owned());
            }
        }
    }
    out
}

/// The word lists the books are made of: the hunt's Arabic, Hebrew
/// and pointed Hebrew, the own and code probes' Arabic, and the Hebrew with
/// Arabic-Indic, Persian and European digits of the round-5 hunt.
pub const AR: [&str; 12] = [
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
];
pub const HE: [&str; 12] = [
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
    "אלהים",
    "הארץ",
];
pub const HEP: [&str; 12] = [
    "שָׁלוֹם",
    "עוֹלָם",
    "מִלְחָמָה",
    "סֵפֶר",
    "בַּיִת",
    "גָּדוֹל",
    "אוֹר",
    "יוֹם",
    "בְּרֵאשִׁית",
    "בָּרָא",
    "אֱלֹהִים",
    "הָאָרֶץ",
];
pub const AR8: [&str; 8] = [
    "عوامل",
    "عدّة",
    "وقد",
    "أصبحنا",
    "نعرف",
    "الآن",
    "الموادّ",
    "تكبح",
];
pub const HEN: [&str; 12] = [
    "שלום",
    "١٢٣",
    "מלחמה",
    "۴۵",
    "בית",
    "٢٠١٠",
    "אור",
    "۷۸۹",
    "בראשית",
    "٣٫٥",
    "אלהים",
    "٦٠",
];

/// `n` sentences, the `k`th made by `f(3 * k)`.
pub fn rep(n: usize, f: impl Fn(usize) -> String) -> String {
    (0..n).map(|k| f(k * 3)).collect::<Vec<_>>().join(" ")
}

/// The `i`th word of a list, round and round.
pub fn word(list: &[&'static str], i: usize) -> &'static str {
    list[i % list.len()]
}

/// What a reader got back from one book.
#[derive(Clone, Debug)]
pub struct Reading {
    pub cons: bool,
    pub logical: bool,
    pub whole: bool,
    pub found: bool,
    /// Glyphs drawn over the glyph before them on its baseline.
    pub overprint: usize,
    /// The pages' plain text, a page a line.
    pub text: String,
}

/// The verdicts' names, in the order a reading lists them.
pub const VERDICTS: [&str; 5] = ["cons", "logical", "whole", "found", "apart"];

impl Reading {
    /// Each verdict and whether it holds.
    pub fn verdicts(&self) -> [(&'static str, bool); 5] {
        [
            ("cons", self.cons),
            ("logical", self.logical),
            ("whole", self.whole),
            ("found", self.found),
            ("apart", self.overprint == 0),
        ]
    }

    /// The verdicts that hold, by name, space-separated.
    pub fn holding(&self) -> String {
        self.verdicts()
            .iter()
            .filter(|(_, holds)| *holds)
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Builds the book and reads it.
///
/// # Panics
/// If the book does not open.
pub fn read(language: &str, style: &str, body: &str) -> Reading {
    let bytes = styled_book(language, style, body);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let mut text = String::new();
    let mut texts = Vec::new();
    let mut overprint = 0;
    for at in 0..doc.page_count() {
        let page = doc.page(at).expect("a page");
        let t = page.text();
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&t.plain_text());
        texts.push(t);
        overprint += drawn_over(&page.text_with(&TextOptions {
            content_order: true,
        }));
    }
    let segs = segments(body);
    let full = norm(&text);
    let whole = segs.iter().all(|s| full.contains(s.as_str()));
    let found = tokens(&segs)
        .iter()
        .all(|t| texts.iter().any(|x| !x.search(t).is_empty()));
    Reading {
        cons: conservation(&bytes, &doc).holds(),
        logical: conservation_in_logical_order(&bytes, &doc).holds(),
        whole,
        found,
        overprint,
        text,
    }
}

/// The count of glyphs drawn over another: the glyphs of a page drawn
/// in content order, marks aside, sorted along each baseline, and each one
/// that starts more than a fiftieth of its size before the one left of it
/// ends.
fn drawn_over(drawn: &tinker_pdf::TextPage) -> usize {
    let mut boxes: Vec<(i64, f64, f64, f64)> = Vec::new();
    for line in drawn.lines() {
        for c in &line.chars {
            if c.text.chars().next().is_some_and(nsm) || c.text.trim().is_empty() {
                continue;
            }
            let (x0, _, x1, _) = c.quad.bounds();
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a baseline in tenths of a point"
            )]
            let baseline = (c.origin.1 * 10.0).round() as i64;
            boxes.push((baseline, x0, x1, c.size));
        }
    }
    boxes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    boxes
        .windows(2)
        .filter(|pair| pair[0].0 == pair[1].0 && pair[1].1 < pair[0].2 - 0.02 * pair[1].3)
        .count()
}

/// What a case holds its reading to in one build.
#[derive(Clone, Copy, Debug)]
pub enum Expect {
    /// The verdicts named hold; the others are not asserted. A case that
    /// was fixed, held to its right reading.
    Holds(&'static str),
    /// Exactly the verdicts named hold, and no other. A case still open,
    /// pinned at today's reading, so that a change either way fails the
    /// test: a gain is the limit closing, and its row moves to `Holds` with
    /// the ROADMAP row it names narrowed in the same commit.
    Pinned(&'static str),
    /// Not asserted in this build: the case is the other build's.
    Neither,
}

/// One book a finding named, and what it reads in each build.
#[derive(Clone, Debug)]
pub struct Case {
    /// What the book is, and the finding that named it.
    pub name: &'static str,
    pub language: &'static str,
    pub style: &'static str,
    pub body: String,
    pub default: Expect,
    pub bundled: Expect,
}

impl Case {
    /// A case with the same expectation in both builds.
    pub fn both(
        name: &'static str,
        language: &'static str,
        style: &'static str,
        body: impl Into<String>,
        expect: Expect,
    ) -> Self {
        Self::each(name, language, style, body, expect, expect)
    }

    /// A case with an expectation for each build.
    pub fn each(
        name: &'static str,
        language: &'static str,
        style: &'static str,
        body: impl Into<String>,
        default: Expect,
        bundled: Expect,
    ) -> Self {
        Self {
            name,
            language,
            style,
            body: body.into(),
            default,
            bundled,
        }
    }

    /// This build's expectation.
    pub fn expect(&self) -> Expect {
        if cfg!(feature = "bundled-fonts") {
            self.bundled
        } else {
            self.default
        }
    }
}

/// Whether `reading` is what `expect` asks, and if not, why.
///
/// # Panics
/// On a verdict name that is none of [`VERDICTS`].
pub fn meets(expect: Expect, reading: &Reading) -> Result<(), String> {
    fn named(list: &str) -> Vec<&str> {
        let names: Vec<&str> = list.split_whitespace().collect();
        for name in &names {
            assert!(VERDICTS.contains(name), "no verdict {name:?}");
        }
        names
    }
    match expect {
        Expect::Neither => Ok(()),
        Expect::Holds(list) => {
            let missing: Vec<&str> = named(list)
                .into_iter()
                .filter(|name| {
                    !reading
                        .verdicts()
                        .iter()
                        .any(|(n, holds)| n == name && *holds)
                })
                .collect();
            if missing.is_empty() {
                Ok(())
            } else {
                Err(format!("does not hold {}", missing.join(" ")))
            }
        }
        Expect::Pinned(list) => {
            let want = named(list);
            let now = reading.holding();
            let now: Vec<&str> = now.split_whitespace().collect();
            if now
                == VERDICTS
                    .iter()
                    .copied()
                    .filter(|v| want.contains(v))
                    .collect::<Vec<_>>()
            {
                Ok(())
            } else {
                Err(format!("pinned at [{list}], reads [{}]", now.join(" ")))
            }
        }
    }
}

/// Reads every case of this build and fails, naming each, if any reads
/// otherwise than its expectation.
///
/// # Panics
/// When a case does not read as it is held to.
pub fn check(cases: &[Case]) {
    let mut wrong = Vec::new();
    for case in cases {
        let expect = case.expect();
        if matches!(expect, Expect::Neither) {
            continue;
        }
        let reading = read(case.language, case.style, &case.body);
        println!(
            "CASE\t{}\t{}\t{}\t{}\t[{}]\toverprint={}\t{:?}",
            case.name,
            case.language,
            case.style,
            case.body,
            reading.holding(),
            reading.overprint,
            reading.text
        );
        if let Err(why) = meets(expect, &reading) {
            wrong.push(format!(
                "{}: {why}\n    {:?} {:?}\n    {}\n    reads {:?}",
                case.name, case.language, case.style, case.body, reading.text
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} cases read otherwise:\n{}",
        wrong.len(),
        cases.len(),
        wrong.join("\n")
    );
}
