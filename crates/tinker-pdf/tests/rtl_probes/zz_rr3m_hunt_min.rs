//! `zz_rr3m_hunt_min` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_min.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family min.

use super::common::{run_book, Case};

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
        }
    }
    emit!("books={n}");
}

fn cases() -> Vec<Case> {
    Vec::new()
}

fn styles() -> Vec<String> {
    Vec::new()
}

/// The minimised books, each with its own style.
fn min() {
    let books: [(&str, &str, &str, &str); 12] = [
        // A: an ET after a European number opening a wrapped Arabic line.
        (
            "body { font-family: sans-serif } p { margin: 0 42% }",
            "A1-percent",
            "ar",
            "<p dir=\"rtl\">جميل بيت كبير نور 5% يوم</p>",
        ),
        (
            "body { font-family: sans-serif } p { margin: 0 42% }",
            "A2-permille",
            "ar",
            "<p dir=\"rtl\">جميل بيت كبير نور 5‰ يوم</p>",
        ),
        (
            "body { font-family: sans-serif } p { margin: 0 46% }",
            "A3-permille-46",
            "ar",
            "<p dir=\"rtl\">جميل بيت كبير نور 5‰ يوم</p>",
        ),
        (
            "body { font-family: sans-serif } p { margin: 0 42% }",
            "A4-range",
            "ar",
            "<p dir=\"rtl\">جميل بيت كبير نور 10–20% يوم</p>",
        ),
        // B: a dir-less paragraph's wrapped last line `(W) W.`, its brackets styled apart.
        (
            "p { margin: 0 42% }",
            "B1-ar",
            "ar",
            "<p>جميل بيت كبير <b>(</b>نور<b>)</b> يوم.</p>",
        ),
        (
            "p { margin: 0 42% }",
            "B2-he",
            "he",
            "<p>שלום עולם ספר <b>(</b>בית<b>)</b> יום.</p>",
        ),
        (
            "p { margin: 0 42% }",
            "B3-he-long",
            "he",
            "<p>שלום עולם ספר בית יום אור יום <b>(</b>בית<b>)</b> יום.</p>",
        ),
        // C: a styled `?!` glued to the right-to-left word after it, letter-spaced and justified.
        (
            "p { letter-spacing: 0.25em; text-align: justify; margin: 0 25% }",
            "C1-qx",
            "ar",
            "<p dir=\"rtl\">كتاب <i>?!</i> مدرسة <b>?!</b>الطالب جميل.</p>",
        ),
        // D: a styled run of neutrals glued to an Arabic word whose code-32 letter moves (CD-19 drawing, unchanged), read into the word.
        (
            "p { word-spacing: 0.6em }",
            "D1-dot",
            "ar",
            "<p dir=\"rtl\">ا <b>.</b>أصبحنا ن</p>",
        ),
        (
            "p { word-spacing: 0.6em }",
            "D2-dots",
            "ar",
            "<p dir=\"rtl\">ال <i>...</i> وقد <b>...</b>أصبحنا نعرف.</p>",
        ),
        // E: a styled ARABIC PERCENT SIGN opening a justified line.
        (
            "p { text-align: justify; margin: 0 41% }",
            "E1-arpct",
            "ar",
            "<p dir=\"rtl\">الطالب ﴿جميل﴾. جميل <b>٪</b>٥٠ بيت <i>٫</i> كبير ﴿نور﴾.</p>",
        ),
        // Control: the same percent in Hebrew does not regress.
        (
            "body { font-family: sans-serif } p { margin: 0 42% }",
            "Z-he-percent",
            "he",
            "<p dir=\"rtl\">שלום עולם ספר בית 5% יום</p>",
        ),
    ];
    for (style, name, lang, body) in books {
        run_book(style, name, lang, body);
    }
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
    min();
}
