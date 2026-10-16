//! The harness the families share: every item two or more of the kit's probes
//! carried byte for byte.

use crate::epub_support::book::styled_book;
use crate::epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::{Document, TextOptions};
use tinker_pdf_shape::unicode::{bidi_class, BidiClass};

pub(crate) fn hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub(crate) const KATABA: &str = "\u{643}\u{64E}\u{62A}\u{64E}\u{628}\u{64E}";

pub(crate) const AMALIYYAN: &str = "\u{639}\u{645}\u{644}\u{64A}\u{651}\u{64B}\u{627}\u{60C} \
    \u{62A}\u{62A}\u{623}\u{644}\u{651}\u{641} \u{643}\u{644}\u{651} \
    \u{627}\u{644}\u{645}\u{648}\u{627}\u{62F} \u{645}\u{646} \
    \u{630}\u{631}\u{651}\u{627}\u{62A}.";

pub(crate) const MUDDA_MUALLIM: &str =
    "\u{645}\u{64F}\u{62F}\u{651}\u{64E}\u{629} \u{645}\u{64F}\u{639}\u{644}\u{651}\u{650}\u{645}.";

pub(crate) const ALLAH3: &str = "\u{671}\u{644}\u{644}\u{651}\u{64E}\u{670}\u{647}\u{650}";

pub(crate) const SHALOM: &str = "\u{5E9}\u{5B8}\u{5C1}\u{5DC}\u{5D5}\u{5B9}\u{5DD}";

pub(crate) const MELEKH: &str = "\u{5DE}\u{5B6}\u{5DC}\u{5B6}\u{5DA}\u{5B0}";

pub(crate) const HASHABBAT: &str =
    "\u{5D4}\u{5B7}\u{5E9}\u{5BC}\u{5C1}\u{5B7}\u{5D1}\u{5BC}\u{5B8}\u{5EA}";

pub(crate) const BERESHIT: &str =
    "\u{5D1}\u{5BC}\u{5B0}\u{5E8}\u{5B5}\u{5D0}\u{5E9}\u{5C1}\u{5B4}\u{5D9}\u{5EA}";

pub(crate) const HET_VAV: &str = "\u{5D7}\u{5D5}";

pub(crate) const CANTILLATED: &str = "\u{5E9}\u{5BC}\u{5C1}\u{5B8}\u{591}\u{5DC}\u{5B5}\u{5DD}";

pub(crate) const LATIN: &str =
    "cafe\u{301} na\u{308}ive Nguye\u{302}\u{303}n q\u{307}\u{323} o\u{302}\u{301}";

pub(crate) const GRCY: &str = "\u{3B1}\u{301}\u{3BB}\u{3C6}\u{3B1} \u{430}\u{301}\u{431}";

pub(crate) const VIET: &str = "Nguye\u{302}\u{303}n Tha\u{300}nh ca\u{301}c ba\u{323}n";

pub(crate) fn esc(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c.to_string()
            } else if c == '\n' {
                "\\n".to_owned()
            } else {
                format!("\\u{{{:x}}}", c as u32)
            }
        })
        .collect()
}

pub(crate) fn nsm(c: char) -> bool {
    bidi_class(c) == BidiClass::NSM
}

pub(crate) fn rtl(c: char) -> bool {
    matches!(bidi_class(c), BidiClass::R | BidiClass::AL)
}

pub(crate) fn strip(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0591}'..='\u{05C7}'))
        .collect()
}

pub(crate) fn untag(s: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in s.chars() {
        match c {
            '<' => inside = true,
            '>' => {
                inside = false;
                out.push(' ');
            }
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out
}

pub(crate) type Book = (String, &'static str, String);

/// One shape of a family's sentences: its name, and the text from a word index.
pub(crate) type Shape<'a> = (&'a str, Box<dyn Fn(usize) -> String + 'a>);

/// (name, language, body)
pub(crate) type Case = (String, &'static str, String);

pub(crate) const HE: [&str; 12] = [
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

pub(crate) const HEP: [&str; 12] = [
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

pub(crate) const AR: [&str; 12] = [
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

pub(crate) const ARP: [&str; 12] = [
    "كِتَابٌ",
    "مَدْرَسَةٌ",
    "الطَّالِبُ",
    "جَمِيلٌ",
    "بَيْتٌ",
    "كَبِيرٌ",
    "نُورٌ",
    "يَوْمٌ",
    "عَوَامِلُ",
    "وَقَدْ",
    "أَصْبَحْنَا",
    "نَعْرِفُ",
];

pub(crate) fn format_char(c: char) -> bool {
    matches!(c, '\u{AD}' | '\u{61C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

pub(crate) fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| !format_char(*c))
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn strip_marks(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0591}'..='\u{05C7}'))
        .collect()
}

pub(crate) const BLOCKS: [&str; 26] = [
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
pub(crate) fn segments(body: &str) -> Vec<String> {
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

pub(crate) fn tokens(segs: &[String]) -> Vec<String> {
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

pub(crate) fn run_book(style: &str, name: &str, lang: &str, body: &str) {
    let bytes = styled_book(lang, style, body);
    let doc = match Document::open(bytes.clone()) {
        Ok(doc) => doc,
        Err(e) => {
            emit!("ROW\t{style}\t{name}\tm=0\ttext=0\tsearch=0/1\tltr=-\tcons=0\tlogical=0\tOPEN-ERR {}", esc(&format!("{e:?}")));
            return;
        }
    };
    let cos_pages = |doc: &Document| -> u64 {
        let cos = doc.cos();
        let mut all = Vec::new();
        for page in tinker_pdf_cos::pages::collect(cos) {
            all.extend_from_slice(&tinker_pdf_cos::pages::content_bytes(cos, &page));
        }
        hash(&all)
    };
    let marked = body.chars().any(nsm);
    let mut text = String::new();
    let mut lines = 0;
    let mut outside = 0usize;
    let mut overlaps = 0usize;
    let mut maxx = f64::MIN;
    let mut vov = 0usize;
    let mut vov_at = String::new();
    let mut texts = Vec::new();
    for at in 0..doc.page_count() {
        let page = doc.page(at).expect("a page");
        let t = page.text();
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&t.plain_text());
        texts.push(t);
        let drawn = page.text_with(&TextOptions {
            content_order: true,
        });
        lines += drawn.lines().len();
        let mut boxes: Vec<(i64, f64, f64, f64, String)> = Vec::new();
        for line in drawn.lines() {
            let mut base: Option<&tinker_pdf::TextChar> = None;
            let mut prev: Option<&tinker_pdf::TextChar> = None;
            for c in &line.chars {
                let (x0, _, x1, _) = c.quad.bounds();
                maxx = maxx.max(x1);
                let mark = c.text.chars().next().is_some_and(nsm);
                if mark {
                    match base {
                        Some(b) => {
                            let centre = (x0 + x1) / 2.0;
                            let (b0, _, b1, _) = b.quad.bounds();
                            if !(b0 < centre && centre < b1) {
                                outside += 1;
                            }
                        }
                        None => outside += 1,
                    }
                    continue;
                }
                if let Some(p) = prev {
                    let (_, _, p1, _) = p.quad.bounds();
                    if x0 < p1 - 0.02 * c.size && x0 > p1 - 3.0 * c.size {
                        overlaps += 1;
                    }
                }
                base = Some(c);
                prev = Some(c);
                if !c.text.trim().is_empty() {
                    boxes.push((
                        (c.origin.1 * 10.0).round() as i64,
                        x0,
                        x1,
                        c.size,
                        c.text.clone(),
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
    let segs = segments(body);
    let full = norm(&text);
    let text_ok = if segs.is_empty() {
        "-".to_owned()
    } else {
        (segs.iter().all(|s| full.contains(s.as_str())) as u8).to_string()
    };
    let toks = tokens(&segs);
    let found_in = |t: &str| texts.iter().any(|x| !x.search(t).is_empty());
    let found = if toks.is_empty() {
        "-".to_owned()
    } else {
        format!(
            "{}/{}",
            toks.iter().filter(|t| found_in(t)).count(),
            toks.len()
        )
    };
    let ltr_toks: Vec<&String> = toks.iter().filter(|t| !t.chars().any(rtl)).collect();
    let ltr = if ltr_toks.is_empty() {
        "-".to_owned()
    } else {
        format!(
            "{}/{}",
            ltr_toks.iter().filter(|t| found_in(t)).count(),
            ltr_toks.len()
        )
    };
    let c = conservation(&bytes, &doc);
    let l = conservation_in_logical_order(&bytes, &doc);
    let first = |v: &crate::epub_support::conservation::Verdict| {
        v.divergences
            .first()
            .map(|d| esc(&format!("{d:?}")))
            .unwrap_or_default()
    };
    let missing: Vec<&String> = toks.iter().filter(|t| !found_in(t)).take(4).collect();
    emit!(
        "ROW\t{style}\t{name}\tm={}\ttext={text_ok}\tsearch={found}\tltr={ltr}\tcons={}\tlogical={}\tride={outside}\toverlap={overlaps}/{vov}\tlines={lines}\tmaxx={maxx:.3}\tthash={:016x}\tchash={:016x}\t{}\t{}\t{}\tnf={}",
        marked as u8,
        c.holds() as u8,
        l.holds() as u8,
        hash(text.as_bytes()),
        cos_pages(&doc),
        if text.chars().count() < 1200 { esc(&text) } else { String::from("<long>") },
        first(&c),
        first(&l),
        esc(&missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",")),
    );
    if vov > 0 {
        emit!("VOV\t{style}\t{name}\t{vov}\t{vov_at}");
    }
}

pub(crate) fn rep(n: usize, f: impl Fn(usize) -> String) -> String {
    (0..n).map(|k| f(k * 3)).collect::<Vec<_>>().join(" ")
}

pub(crate) const GR: [&str; 12] = [
    "αλφα",
    "βήτα",
    "γάμμα",
    "δέλτα",
    "έψιλον",
    "ζήτα",
    "ήτα",
    "θήτα",
    "ιώτα",
    "κάππα",
    "λάμδα",
    "μυ",
];

pub(crate) fn halves(w: &str) -> (String, String) {
    let v: Vec<char> = w.chars().collect();
    let k = (v.len() / 2).max(1).min(v.len());
    (v[..k].iter().collect(), v[k..].iter().collect())
}

pub(crate) const CY: [&str; 12] = [
    "привет",
    "мир",
    "книга",
    "дом",
    "большой",
    "свет",
    "день",
    "работа",
    "знать",
    "слово",
    "город",
    "вода",
];
