//! `zz_vx_k11rv_neutral` (vx, round-5 review (confirm)): the books of
//! `tools/probes/rtl/probes/pdf/zz_vx_k11rv_neutral.rs`, which is the probe as it ran.
//!
//! Re-review probe: runs of neutrals at a
//! right-to-left level, which `coded_order` now draws in level order on a line
//! that holds a right-to-left character (or an RLE/RLO/RLI). One ROW per book.

use super::vov::styled_book;
use crate::epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::Document;

fn probe() {
    let bodies: Vec<(&str, &str, String)> = vec![
        (
            "rli-chars",
            "en",
            "<p>x \u{2067}?!\u{2069} y</p>".to_owned(),
        ),
        (
            "rle-chars",
            "en",
            "<p>x \u{202B}?!\u{202C} y</p>".to_owned(),
        ),
        (
            "rlo-chars",
            "en",
            "<p>x \u{202E}?!\u{202C} y</p>".to_owned(),
        ),
        (
            "rli-chars-he",
            "he",
            "<p>x \u{2067}?!\u{2069} y שלום</p>".to_owned(),
        ),
        (
            "span-rtl-latin",
            "en",
            "<p>abc <span dir=\"rtl\">?!</span> def</p>".to_owned(),
        ),
        (
            "span-rtl-he",
            "en",
            "<p>abc <span dir=\"rtl\">?!</span> שלום</p>".to_owned(),
        ),
        (
            "rtl-span-ltr",
            "he",
            "<p dir=\"rtl\">שלום <span dir=\"ltr\">?!</span> עולם</p>".to_owned(),
        ),
        (
            "rtl-mid",
            "he",
            "<p dir=\"rtl\">שלום ?! עולם</p>".to_owned(),
        ),
        (
            "rtl-italic-mid",
            "he",
            "<p dir=\"rtl\">שלום <i>?!</i> עולם</p>".to_owned(),
        ),
        ("rtl-opens", "he", "<p dir=\"rtl\">?! שלום</p>".to_owned()),
        (
            "rtl-italic-opens",
            "he",
            "<p dir=\"rtl\"><i>?!</i> שלום</p>".to_owned(),
        ),
        (
            "rtl-dots-opens",
            "he",
            "<p dir=\"rtl\">(...) שלום</p>".to_owned(),
        ),
        (
            "rtl-bold-brackets",
            "he",
            "<p dir=\"rtl\">שלום <b>(</b>abc<b>)</b> עולם.</p>".to_owned(),
        ),
        (
            "rtl-bold-brackets-ar",
            "ar",
            "<p dir=\"rtl\">مرحبا <b>(</b>abc<b>)</b> عالم.</p>".to_owned(),
        ),
        (
            "rtl-italic-ellipsis",
            "he",
            "<p dir=\"rtl\">שלום <i>...</i> עולם</p>".to_owned(),
        ),
        (
            "rtl-italic-quotes",
            "he",
            "<p dir=\"rtl\">שלום <i>\"</i>abc<i>\"</i></p>".to_owned(),
        ),
        (
            "rtl-italic-number-dot",
            "he",
            "<p dir=\"rtl\"><i>1.</i> שלום</p>".to_owned(),
        ),
        (
            "rtl-italic-time",
            "he",
            "<p dir=\"rtl\">שלום <i>12:30</i> עולם</p>".to_owned(),
        ),
        (
            "rtl-italic-dashes",
            "he",
            "<p dir=\"rtl\">שלום <i>- -</i> עולם</p>".to_owned(),
        ),
        (
            "rtl-italic-trailing",
            "he",
            "<p dir=\"rtl\">שלום<i>!?</i></p>".to_owned(),
        ),
        (
            "ltr-italic-before-he",
            "he",
            "<p>Hello <i>?!</i> שלום</p>".to_owned(),
        ),
        (
            "rtl-heading",
            "he",
            "<h1 dir=\"rtl\">?!</h1><p dir=\"rtl\">שלום עולם</p>".to_owned(),
        ),
        ("rtl-br", "he", "<p dir=\"rtl\">?!<br/>שלום</p>".to_owned()),
        (
            "rtl-br-after",
            "he",
            "<p dir=\"rtl\">שלום<br/>?!</p>".to_owned(),
        ),
        (
            "rtl-pct",
            "ar",
            "<p dir=\"rtl\">نسبة <i>%</i>50 عالية</p>".to_owned(),
        ),
        (
            "rtl-italic-paren-num",
            "he",
            "<p dir=\"rtl\"><i>(1)</i> פריט</p>".to_owned(),
        ),
        (
            "rtl-li",
            "he",
            "<ol dir=\"rtl\" style=\"list-style-position: inside\"><li>שלום ?!</li></ol>"
                .to_owned(),
        ),
        (
            "rtl-italic-colon",
            "ar",
            "<p dir=\"rtl\">قال<i>: «</i>مرحبا<i>»</i></p>".to_owned(),
        ),
        (
            "rtl-italic-plus",
            "he",
            "<p dir=\"rtl\">שלום <i>+ -</i> 5 עולם</p>".to_owned(),
        ),
        (
            "rtl-italic-slash",
            "ar",
            "<p dir=\"rtl\">مرحبا <i>/</i> عالم <i>//</i> هنا</p>".to_owned(),
        ),
    ];
    let styles = [
        "",
        "body { font-family: sans-serif }",
        "p { text-align: justify; margin: 0 40% }",
    ];
    for (si, style) in styles.iter().enumerate() {
        for (name, lang, body) in &bodies {
            let bytes = styled_book(lang, style, body);
            let doc = match Document::open(bytes.clone()) {
                Ok(d) => d,
                Err(e) => {
                    emit!("ROW\ts{si}\t{name}\tOPEN-ERR {e:?}");
                    continue;
                }
            };
            let c = conservation(&bytes, &doc);
            let l = conservation_in_logical_order(&bytes, &doc);
            let text: Vec<String> = (0..doc.page_count())
                .filter_map(|p| doc.page(p))
                .map(|p| p.text().plain_text())
                .collect();
            emit!(
                "ROW\ts{si}\t{name}\tcontent={}\tlogical={}\t{:?}",
                c.holds(),
                l.holds(),
                text.join("|")
            );
        }
    }
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
