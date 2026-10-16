//! `zz_rr3m_h1min` (h1, round-5 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h1min.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11/CD-19 round-5 hunt, minimised books.

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
    // A: a paragraph's last stop after Hebrew text and a number (C2).
    let a_styles = [
        "p { word-spacing: 0.5em; margin: 0 20% }",
        "p { word-spacing: 0.5em }",
        "p { word-spacing: 1em }",
        "p { word-spacing: 0.3em }",
        "body { font-family: monospace } p { word-spacing: 0.5em; text-align: justify; margin: 0 25% }",
    ];
    let a_books: [(&str, &'static str, &str); 14] = [
        ("a-full-hen", "he", "<p>שלום ١٢٣ מלחמה, ۴۵ בית ٢٠١٠. ۴۵ בית ٢٠١٠, אור ۷۸۹ בראשית. אור ۷۸۹ בראשית, ٣٫٥ אלהים ٦٠.</p>"),
        ("a-last-hen", "he", "<p>אור ۷۸۹ בראשית, ٣٫٥ אלהים ٦٠.</p>"),
        ("a-shin-an", "he", "<p>שלום אלהים ٦٠.</p>"),
        ("a-shin-an2", "he", "<p>שלום ٦٠.</p>"),
        ("a-shin-word", "he", "<p>שלום אלהים.</p>"),
        ("a-shin-word-ltr", "he", "<p dir=\"ltr\">שלום אלהים.</p>"),
        ("a-bereshit", "he", "<p>בראשית ברא.</p>"),
        ("a-bereshit-ltr", "he", "<p dir=\"ltr\">בראשית ברא.</p>"),
        ("a-styled-last", "he", "<p>אור <b>יום</b> abc בראשית 2010 <i>ברא</i>, (אלהים).</p>"),
        ("a-styled-last-ltr", "he", "<p dir=\"ltr\">אור <b>יום</b> abc בראשית 2010 <i>ברא</i>, (אלהים).</p>"),
        ("a-paren", "he", "<p>שלום בראשית 2010 ברא, (אלהים).</p>"),
        ("a-paren-ltr", "he", "<p dir=\"ltr\">שלום בראשית 2010 ברא, (אלהים).</p>"),
        ("a-two-lines", "he", "<p>שלום עולם מלחמה ספר בית גדול אור יום בראשית ברא אלהים הארץ שלום עולם מלחמה ספר.</p>"),
        ("a-two-lines-ltr", "he", "<p dir=\"ltr\">שלום עולם מלחמה ספר בית גדול אור יום בראשית ברא אלהים הארץ שלום עולם מלחמה ספר.</p>"),
    ];
    for st in a_styles {
        for (name, lang, body) in a_books {
            out.push((st.to_owned(), name.to_owned(), lang, body.to_owned()));
        }
    }
    // B: a dir-less table row, a right-to-left cell then a Greek one (C2).
    let b_styles = [
        "table { border-spacing: 0 } td { padding: 0; word-spacing: 1em }",
        "table { border-spacing: 0 } td { padding: 0; word-spacing: 0.5em }",
        "table { border-spacing: 0 } td { padding: 0; letter-spacing: 0.1em; word-spacing: 0.5em }",
        "td { word-spacing: 1em }",
    ];
    let b_books: [(&str, &'static str, &str); 10] = [
        ("b-full-arnum", "ar", "<table><tr><td>αλφα βήτα</td><td>كتاب ١٢ نور</td></tr><tr><td>كتاب ١٢ نور</td><td>αλφα βήτα</td></tr></table>"),
        ("b-row2-arnum", "ar", "<table><tr><td>كتاب ١٢ نور</td><td>αλφα βήτα</td></tr></table>"),
        ("b-row2-ar", "ar", "<table><tr><td>كتاب نور</td><td>αλφα βήτα</td></tr></table>"),
        ("b-row2-ar1", "ar", "<table><tr><td>نور</td><td>αλφα βήτα</td></tr></table>"),
        ("b-row2-he", "he", "<table><tr><td>ספר בית</td><td>αλφα βήτα</td></tr></table>"),
        ("b-row2-hep", "he", "<table><tr><td>סֵפֶר בַּיִת</td><td>αλφα βήτα</td></tr></table>"),
        ("b-row2-cy", "ar", "<table><tr><td>كتاب نور</td><td>книга дом</td></tr></table>"),
        ("b-row2-arneu", "ar", "<table><tr><td>(كتاب) نور.</td><td>αλφα βήτα</td></tr></table>"),
        ("b-p", "ar", "<p>كتاب نور αλφα βήτα</p>"),
        ("b-full-hep", "he", "<table><tr><td>αλφα βήτα</td><td>סֵפֶר בַּיִת</td></tr><tr><td>סֵפֶר בַּיִת</td><td>αλφα βήτα</td></tr></table>"),
    ];
    for st in b_styles {
        for (name, lang, body) in b_books {
            out.push((st.to_owned(), name.to_owned(), lang, body.to_owned()));
        }
    }
    // C: a number, an ARABIC COMMA or angle bracket, a bold Hebrew word (C3, bundled-fonts).
    let c_styles = [
        "p { letter-spacing: 0.1em; word-spacing: 0.25em; text-align: justify; margin: 0 35% }",
        "p { letter-spacing: 0.1em; word-spacing: 0.25em }",
        "p { word-spacing: 0.25em }",
        "p { word-spacing: 0.5em }",
        "p { text-align: justify; margin: 0 35% }",
    ];
    let c_books: [(&str, &'static str, &str); 10] = [
        (
            "c-full-acomma",
            "he",
            "<p>שלום בראשית 25، <b>עולם</b> מלחמה.</p>",
        ),
        (
            "c-full-angle",
            "he",
            "<p>שלום בראשית ⟨25⟩ <b>עולם</b> מלחמה.</p>",
        ),
        (
            "c-acomma-rtl",
            "he",
            "<p dir=\"rtl\">שלום בראשית 25، <b>עולם</b> מלחמה.</p>",
        ),
        ("c-short-acomma", "he", "<p>שלום 25، <b>עולם</b> מלחמה.</p>"),
        ("c-short2-acomma", "he", "<p>שלום 25، <b>עולם</b>.</p>"),
        (
            "c-nobold-acomma",
            "he",
            "<p>שלום בראשית 25، עולם מלחמה.</p>",
        ),
        (
            "c-full-pct",
            "he",
            "<p>שלום בראשית 25٪ <b>עולם</b> מלחמה.</p>",
        ),
        (
            "c-ar-acomma",
            "ar",
            "<p>كتاب عوامل 25، <b>مدرسة</b> الطالب.</p>",
        ),
        (
            "c-an-acomma",
            "he",
            "<p>שלום בראשית ٢٥، <b>עולם</b> מלחמה.</p>",
        ),
        ("c-short-angle", "he", "<p>שלום ⟨25⟩ <b>עולם</b> מלחמה.</p>"),
    ];
    for st in c_styles {
        for (name, lang, body) in c_books {
            out.push((st.to_owned(), name.to_owned(), lang, body.to_owned()));
        }
    }
    // D: 25٪ / 25₪ before a bold Arabic word at word-spacing 1em (C3, logical order).
    let d_styles = [
        "p { word-spacing: 1em; margin: 0 20% }",
        "p { word-spacing: 1em }",
        "p { word-spacing: 0.5em }",
    ];
    let d_books: [(&str, &'static str, &str); 6] = [
        (
            "d-full-pct",
            "ar",
            "<p>كتاب عوامل 25٪ <b>مدرسة</b> الطالب.</p>",
        ),
        (
            "d-full-shekel",
            "ar",
            "<p>كتاب عوامل 25₪ <b>مدرسة</b> الطالب.</p>",
        ),
        ("d-short-pct", "ar", "<p>كتاب 25٪ <b>مدرسة</b> الطالب.</p>"),
        ("d-short2-pct", "ar", "<p>كتاب 25٪ <b>مدرسة</b>.</p>"),
        ("d-nobold-pct", "ar", "<p>كتاب عوامل 25٪ مدرسة الطالب.</p>"),
        (
            "d-rtl-pct",
            "ar",
            "<p dir=\"rtl\">كتاب 25٪ <b>مدرسة</b> الطالب.</p>",
        ),
    ];
    for st in d_styles {
        for (name, lang, body) in d_books {
            out.push((st.to_owned(), name.to_owned(), lang, body.to_owned()));
        }
    }
    // E: a stop after an Arabic-Indic number before an Extended one, read after the space (since K-11).
    let e_styles = [
        "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify; margin: 0 25% }",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em }",
        "p { word-spacing: 0.5em }",
    ];
    let e_books: [(&str, &'static str, &str); 5] = [
        ("e-full", "he", "<p>ף</p><p dir=\"rtl\">שלום ١٢٣ מלחמה, ۴۵ בית ٢٠١٠. ۴۵ בית ٢٠١٠, אור ۷۸۹ בראשית. אור ۷۸۹ בראשית, ٣٫٥ אלהים ٦٠.</p>"),
        ("e-first", "he", "<p>ף</p><p dir=\"rtl\">שלום ١٢٣ מלחמה, ۴۵ בית ٢٠١٠. ۴۵ בית ٢٠١٠, אור.</p>"),
        ("e-min", "he", "<p>ף</p><p dir=\"rtl\">בית ٢٠١٠. ۴۵ בית.</p>"),
        ("e-min-nopref", "he", "<p dir=\"rtl\">בית ٢٠١٠. ۴۵ בית.</p>"),
        ("e-min-en", "he", "<p>ף</p><p dir=\"rtl\">בית ٢٠١٠. 45 בית.</p>"),
    ];
    for st in e_styles {
        for (name, lang, body) in e_books {
            out.push((st.to_owned(), name.to_owned(), lang, body.to_owned()));
        }
    }
    // G: stop runs opening a right-to-left line before a styled pair (R's price).
    for st in ["", "p { word-spacing: 0.6em }"] {
        for (sn, end) in [
            ("dotex", ".!"),
            ("exell", "!…"),
            ("qe", "?!"),
            ("dot", "."),
            ("q", "?"),
        ] {
            out.push((
                st.to_owned(),
                format!("g-{sn}-ar"),
                "ar",
                format!("<p dir=\"rtl\">كتاب عوامل<br/>{end}<b>(</b>جميل<b>)</b> بيت نعرف</p>"),
            ));
            out.push((
                st.to_owned(),
                format!("g-{sn}-he"),
                "he",
                format!("<p dir=\"rtl\">שלום בראשית<br/>{end}<b>(</b>ספר<b>)</b> בית הארץ</p>"),
            ));
        }
    }
    out
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
