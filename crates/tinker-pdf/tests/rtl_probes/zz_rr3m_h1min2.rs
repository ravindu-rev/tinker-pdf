//! `zz_rr3m_h1min2` (h1, round-5 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h1min2.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11/CD-19 round-5 hunt, zz_rr3m_h1min2.

use super::common::run_book;

// Minimised books of every regression family the hunt found.

fn probe() {
    let mut n = 0usize;
    for (style, name, lang, body) in books() {
        n += 1;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_book(&style, &name, lang, &body)
        }));
        if r.is_err() {
            emit!("ROW\t{style}\t{name}\tm=0\ttext=0\tsearch=0/1\tltr=-\tcons=0\tlogical=0\tPANIC");
        }
    }
    emit!("books={n}");
}

fn books() -> Vec<(String, String, &'static str, String)> {
    let mut out: Vec<(String, String, &'static str, String)> = Vec::new();
    let a_styles = [
        "p { word-spacing: 0.5em }",
        "p { word-spacing: 0.5em; margin: 0 20% }",
        "p { word-spacing: 1em }",
        "p { text-align: justify; margin: 0 30% }",
    ];
    let a_books: [(&str, &'static str, &str); 12] = [
        (
            "a3-br",
            "he",
            "<p>שלום אור<br/>בראשית. אור בראשית אלהים.</p>",
        ),
        ("a3-two", "he", "<p>בראשית. אור בראשית.</p>"),
        ("a3-two-shin", "he", "<p>ש</p><p>בראשית. אור בראשית.</p>"),
        ("a3-ean", "he", "<p>ש</p><p>בראשית. אור ۷۸۹ בראשית.</p>"),
        (
            "a3-last",
            "he",
            "<p>ש</p><p>בראשית. אור ۷۸۹ בראשית, ٣٫٥ אלהים ٦٠.</p>",
        ),
        ("a3-dot2", "he", "<p>ש</p><p>בראשית. אור.</p>"),
        ("a3-an", "he", "<p>ש</p><p>בראשית. אור ٦٠.</p>"),
        ("a3-nodot", "he", "<p>ש</p><p>בראשית אור.</p>"),
        ("a3-comma", "he", "<p>ש</p><p>בראשית, אור.</p>"),
        ("a3-ar", "ar", "<p>ك</p><p>كتاب. نور كتاب.</p>"),
        ("a3-mid-shin", "he", "<p>ש</p><p>שלום עולם.</p>"),
        ("a3-word", "he", "<p>ש</p><p>בראשית.</p>"),
    ];
    for st in a_styles {
        for (name, lang, body) in a_books {
            out.push((st.to_owned(), name.to_owned(), lang, body.to_owned()));
        }
    }
    let b_styles = [
        "td { word-spacing: 1em }",
        "table { border-spacing: 0 } td { padding: 0; word-spacing: 1em }",
        "td { word-spacing: 0.6em }",
        "td { text-align: justify; word-spacing: 0.5em }",
    ];
    let b_books: [(&str, &'static str, &str); 9] = [
        (
            "b2-gr-he",
            "he",
            "<table><tr><td>αλφα βήτα</td><td>ספר</td></tr></table>",
        ),
        (
            "b2-gr-ar",
            "ar",
            "<table><tr><td>αλφα βήτα</td><td>كتاب</td></tr></table>",
        ),
        (
            "b2-gr2-he",
            "he",
            "<table><tr><td>αβ γδ</td><td>ספר</td></tr></table>",
        ),
        (
            "b2-cy-he",
            "he",
            "<table><tr><td>книга дом</td><td>ספר</td></tr></table>",
        ),
        (
            "b2-x-gr-he",
            "he",
            "<table><tr><td>x αλφα βήτα</td><td>ספר</td></tr></table>",
        ),
        (
            "b2-gr-hep",
            "he",
            "<table><tr><td>αλφα βήτα</td><td>סֵפֶר בַּיִת</td></tr></table>",
        ),
        (
            "b2-gr-he2",
            "he",
            "<table><tr><td>αλφα βήτα</td><td>ספר בית</td></tr></table>",
        ),
        (
            "b2-lat-he",
            "he",
            "<table><tr><td>abc def</td><td>ספר</td></tr></table>",
        ),
        (
            "b2-gr-he-rtl",
            "he",
            "<table dir=\"rtl\"><tr><td>αλφα βήτα</td><td>ספר</td></tr></table>",
        ),
    ];
    for st in b_styles {
        for (name, lang, body) in b_books {
            out.push((st.to_owned(), name.to_owned(), lang, body.to_owned()));
        }
    }
    out
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
