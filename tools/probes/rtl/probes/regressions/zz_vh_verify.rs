#![allow(missing_docs, clippy::all, unused)]

mod epub_support;

use epub_support::book::styled_book;
use epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::Document;

struct Out {
    cons: bool,
    logical: bool,
    plain: String,
    found: Vec<(String, bool)>,
}

fn run(language: &str, style: &str, body: &str, search: &[&str]) -> Out {
    let bytes = styled_book(language, style, body);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let mut plain = String::new();
    let mut texts = Vec::new();
    for at in 0..doc.page_count() {
        let t = doc.page(at).expect("a page").text();
        plain.push_str(&t.plain_text());
        plain.push_str(" ||| ");
        texts.push(t);
    }
    let found = search
        .iter()
        .map(|w| (w.to_string(), texts.iter().any(|t| !t.search(w).is_empty())))
        .collect();
    let c = conservation(&bytes, &doc);
    let l = conservation_in_logical_order(&bytes, &doc);
    let out = Out { cons: c.holds(), logical: l.holds(), plain, found };
    println!(
        "VH\t{language}\t{style}\t{body}\tcons={}\tlogical={}\tfound={:?}\tplain={:?}",
        out.cons, out.logical, out.found, out.plain
    );
    out
}

// H1: an ET after a European number opening a wrapped Arabic line.
#[test]
fn h1_et_after_number_opening_wrapped_arabic_line() {
    let style = "body { font-family: sans-serif } p { margin: 0 42% }";
    let mut bad = Vec::new();
    for body in [
        "<p dir=\"rtl\">جميل بيت كبير نور 5% يوم</p>",
        "<p dir=\"rtl\">جميل بيت كبير نور 5‰ يوم</p>",
        "<p dir=\"rtl\">جميل بيت كبير نور 10–20% يوم</p>",
    ] {
        let o = run("ar", style, body, &[]);
        if !(o.cons && o.logical) {
            bad.push(body);
        }
    }
    // Control (not asserted in the set): Hebrew.
    run("he", style, "<p dir=\"rtl\">שלום עולם ספר בית 5% יום</p>", &[]);
    assert!(bad.is_empty(), "{bad:?}");
}

// H2: dir-less `(W) W.` with brackets styled apart; logical order.
#[test]
fn h2_dirless_styled_brackets_full_stop() {
    let style = "p { margin: 0 42% }";
    let mut bad = Vec::new();
    let books: Vec<(&str, &str)> = if cfg!(feature = "bundled-fonts") {
        vec![
            ("ar", "<p>جميل بيت كبير <b>(</b>نور<b>)</b> يوم.</p>"),
            ("he", "<p>שלום עולם ספר בית יום אור יום <b>(</b>בית<b>)</b> יום.</p>"),
        ]
    } else {
        vec![
            ("ar", "<p>جميل بيت كبير <b>(</b>نور<b>)</b> يوم.</p>"),
            ("he", "<p>שלום עולם ספר <b>(</b>בית<b>)</b> יום.</p>"),
        ]
    };
    for (lang, body) in books {
        let o = run(lang, style, body, &[]);
        if !o.logical {
            bad.push(body);
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
}

// H3: styled `?!` glued to the right-to-left word after it, letter-spaced and justified.
#[test]
fn h3_styled_qx_glued_letter_spaced_justified() {
    let style = "p { letter-spacing: 0.25em; text-align: justify; margin: 0 25% }";
    let o = run("ar", style, "<p dir=\"rtl\">كتاب <i>?!</i> مدرسة <b>?!</b>الطالب جميل.</p>", &[]);
    assert!(o.logical);
}

// H4: CD-19 overprint read into the word; the word no longer found by search.
#[test]
fn h4_word_spaced_overprint_word_found() {
    let style = "p { word-spacing: 0.6em }";
    let mut bad = Vec::new();
    for body in [
        "<p dir=\"rtl\">ا <b>.</b>أصبحنا ن</p>",
        "<p dir=\"rtl\">ال <i>...</i> وقد <b>...</b>أصبحنا نعرف.</p>",
    ] {
        let o = run("ar", style, body, &["أصبحنا"]);
        if !o.found.iter().all(|(_, f)| *f) {
            bad.push(body);
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
}

// H5: a styled ARABIC PERCENT SIGN opening a justified Arabic line.
#[test]
fn h5_styled_arabic_percent_opening_justified_line() {
    let style = "p { text-align: justify; margin: 0 41% }";
    let o = run(
        "ar",
        style,
        "<p dir=\"rtl\">الطالب ﴿جميل﴾. جميل <b>٪</b>٥٠ بيت <i>٫</i> كبير ﴿نور﴾.</p>",
        &[],
    );
    assert!(o.cons);
}
