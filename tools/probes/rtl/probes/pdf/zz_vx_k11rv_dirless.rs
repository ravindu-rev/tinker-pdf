//! Scratch re-review probe (never committed): every line of K-11's dir-less
//! books, classified — read right, misread but drawn alike, or misread and
//! not drawn alike.
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
use epub_support::conservation::conservation_in_logical_order;
use tinker_pdf::Document;
use tinker_pdf_shape::bidi::{logical_order, order_units, BaseDirection};

fn strip(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{064B}'..='\u{065F}' | '\u{0670}'))
        .collect()
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn probe() {
    let fillers = [
        "إنّ الخلايا السرطانيّة تتكاثر بسرعة كبيرة، ويعتمد ذلك على عوامل عدّة",
        "تتألّف كلّ الموادّ الحيّة على كوكبنا الأرض من خلايا، باستثناء الفيروسات",
    ];
    let insides = ["HDAC", "histone deacetylase", "النازعة للأسيتيل", "2010", "انظر الفصل الثالث"];
    let tail = "وقد أصبحنا نعرف الآن أنّ بعض الموادّ تكبح الهستونات النازعة للأسيتيل، مخفّفة بذلك من نموّ الأورام";
    let styles = [
        "",
        "p { text-align: justify }",
        "p { text-align: justify; margin: 0 20% }",
        "p { text-align: justify; margin: 0 35% }",
        "p { margin: 0 25% }",
        "p { text-align: justify; font-size: 1.2em }",
        "p { text-align: justify; margin: 0 10%; line-height: 1.5 }",
        "body { font-family: sans-serif } p { text-align: justify; margin: 0 15% }",
    ];
    let mut tally = std::collections::BTreeMap::<String, usize>::new();
    for style in styles {
        for inside in insides {
            for repeat in [1usize, 3, 6] {
                let mut para = String::new();
                for i in 0..repeat {
                    para.push_str(fillers[i % 2]);
                    para.push_str(" (");
                    para.push_str(inside);
                    para.push_str("). ");
                    para.push_str(tail);
                    para.push_str(". ");
                }
                let text = strip(&para);
                let source = squash(&text);
                let bytes = styled_book("ar", style, &format!("<p>{text}</p>"));
                let doc = Document::open(bytes.clone()).expect("opens");
                let holds = conservation_in_logical_order(&bytes, &doc).holds();
                let mut kinds: Vec<&str> = Vec::new();
                for p in 0..doc.page_count() {
                    let page = doc.page(p).expect("page");
                    let t = page.text();
                    for line in t.lines() {
                        let mut drawn: Vec<(f64, &str)> = line
                            .chars
                            .iter()
                            .map(|c| (c.quad.bounds().0, c.text.as_str()))
                            .collect();
                        drawn.sort_by(|a, b| a.0.total_cmp(&b.0));
                        let visual: Vec<&str> = drawn.iter().map(|(_, s)| *s).collect();
                        let read = |d| -> (String, bool) {
                            let order = logical_order(&visual, d);
                            let r: Vec<&str> = order.iter().map(|at| visual[*at]).collect();
                            let re: Vec<&str> = order_units(&r, d).into_iter().map(|at| r[at]).collect();
                            (r.concat(), re == visual)
                        };
                        let (ltr, ltr_draws) = read(BaseDirection::LeftToRight);
                        let (rtl, rtl_draws) = read(BaseDirection::RightToLeft);
                        let got = squash(&line.text);
                        let right = source.contains(&got);
                        let kind = if right {
                            "right"
                        } else {
                            let other = if squash(&ltr) == got { squash(&rtl) } else { squash(&ltr) };
                            let other_right = source.contains(&other);
                            match (other_right, ltr_draws && rtl_draws) {
                                (true, true) => "misread-other-right-drawn-alike",
                                (true, false) => "MISREAD-OTHER-RIGHT-NOT-DRAWN-ALIKE",
                                (false, true) => "misread-neither-right-drawn-alike",
                                (false, false) => "misread-neither-right-not-drawn-alike",
                            }
                        };
                        *tally.entry(format!("{inside}\t{kind}")).or_default() += 1;
                        if kind != "right" {
                            kinds.push(kind);
                            println!(
                                "LINE\t{style:?}\t{inside}\tx{repeat}\t{kind}\tgot={:?}\tltr={ltr:?}\trtl={rtl:?}",
                                line.text
                            );
                        }
                    }
                }
                println!("BOOK\t{style:?}\t{inside}\tx{repeat}\tholds={holds}\t{kinds:?}");
            }
        }
    }
    for (k, v) in tally {
        println!("TALLY\t{k}\t{v}");
    }
}
