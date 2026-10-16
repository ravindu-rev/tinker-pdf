//! `zz_vx_k11rv_ext` (vx, round-5 review (confirm)): the books of
//! `tools/probes/rtl/probes/pdf/zz_vx_k11rv_ext.rs`, which is the probe as it ran.
//!
//! Re-review probe: K-11's shapes extended —
//! brackets at line starts and ends, nested brackets, quotation marks and
//! guillemets, a bracket closing a paragraph, Hebrew as well as Arabic,
//! numbers with separators, a left-to-right paragraph quoting a right-to-left
//! bracketed run. One ROW per book.

use super::common::strip;
use super::vov::styled_book;
use crate::epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::Document;

const AR_FILL: &str = "إنّ الخلايا السرطانيّة تتكاثر بسرعة كبيرة، ويعتمد ذلك على عوامل عدّة";

const AR_TAIL: &str = "وقد أصبحنا نعرف الآن أنّ بعض الموادّ تكبح الهستونات النازعة للأسيتيل";

const HE_FILL: &str = "בְּרֵאשִׁית בָּרָא אֱלֹהִים אֵת הַשָּׁמַיִם וְאֵת הָאָרֶץ וְהָאָרֶץ הָיְתָה";

const HE_TAIL: &str = "וַיֹּאמֶר אֱלֹהִים יְהִי אוֹר וַיְהִי אוֹר וַיַּרְא אֱלֹהִים אֶת הָאוֹר";

fn paragraphs() -> Vec<(String, &'static str, &'static str, String)> {
    // (name, lang, open tag, paragraph text)
    let mut out = Vec::new();
    let rtl = "<p dir=\"rtl\">";
    for (script, lang, fill, tail) in [
        ("ar", "ar", AR_FILL, AR_TAIL),
        ("he", "he", HE_FILL, HE_TAIL),
    ] {
        let insides: Vec<(&str, String)> = vec![
            ("acr", "(HDAC)".to_owned()),
            ("words", "(histone deacetylase)".to_owned()),
            ("year", "(2010)".to_owned()),
            ("numsep", "(1,234.56)".to_owned()),
            ("numsep-latin", "(USD 1,234.56)".to_owned()),
            (
                "rtl",
                format!("({})", &tail[..tail.char_indices().nth(12).unwrap().0]),
            ),
            ("nested", "(see [HDAC] here)".to_owned()),
            (
                "nested-rtl",
                format!(
                    "({} [HDAC] {})",
                    &fill[..fill.char_indices().nth(8).unwrap().0],
                    &tail[..tail.char_indices().nth(6).unwrap().0]
                ),
            ),
            ("square", "[HDAC]".to_owned()),
            ("quote", "\"HDAC\"".to_owned()),
            ("curly", "“histone deacetylase”".to_owned()),
            ("guillemets", "«histone deacetylase»".to_owned()),
            ("two-pairs", "(HDAC) و (SIRT)".to_owned()),
            ("ref", "[12]".to_owned()),
        ];
        for (iname, inside) in &insides {
            for repeat in [1usize, 4] {
                let mut para = String::new();
                for _ in 0..repeat {
                    para.push_str(fill);
                    para.push(' ');
                    para.push_str(inside);
                    para.push_str(". ");
                    para.push_str(tail);
                    para.push_str(". ");
                }
                out.push((
                    format!("{script}/rtl/{iname}/x{repeat}"),
                    lang,
                    rtl,
                    para.clone(),
                ));
                out.push((
                    format!("{script}/none/{iname}/x{repeat}"),
                    lang,
                    "<p>",
                    para,
                ));
            }
            // A bracket closing the paragraph.
            out.push((
                format!("{script}/rtl/{iname}/closes"),
                lang,
                rtl,
                format!("{fill} {tail} {inside}"),
            ));
            // A bracket opening the paragraph.
            out.push((
                format!("{script}/rtl/{iname}/opens"),
                lang,
                rtl,
                format!("{inside} {fill} {tail}."),
            ));
        }
        // A left-to-right paragraph quoting a right-to-left bracketed run.
        let short_fill = &fill[..fill.char_indices().nth(20).unwrap().0];
        let short_tail = &tail[..tail.char_indices().nth(14).unwrap().0];
        out.push((format!("{script}/en/quote-bracketed"), "en", "<p>", format!(
            "The source says \"{short_fill} ({short_tail})\" and the translation follows in the next paragraph of this chapter."
        )));
        out.push((format!("{script}/en/quote-bracketed-latin"), "en", "<p>", format!(
            "The source says \"{short_fill} (HDAC). {short_tail}\" and the translation follows in the next paragraph."
        )));
        out.push((format!("{script}/en/two-glosses"), "en", "<p>", format!(
            "The words peace ({}) and war ({}) are glossed here, with more English text after them to fill the line.",
            &short_fill[..short_fill.char_indices().nth(5).unwrap().0],
            &short_tail[..short_tail.char_indices().nth(5).unwrap().0]
        )));
        out.push((
            format!("{script}/en/list"),
            "en",
            "<p>",
            format!(
                "Step 1) read the word ({}) aloud and 2) write it down, then 3) check ({}) again.",
                &short_fill[..short_fill.char_indices().nth(5).unwrap().0],
                &short_tail[..short_tail.char_indices().nth(5).unwrap().0]
            ),
        ));
        out.push((format!("{script}/en/two-glosses-wrap"), "en", "<p>", format!(
            "The words peace ({short_fill}) and war ({short_tail}) are glossed here, and then the English text goes on for a while longer."
        )));
        out.push((format!("{script}/en/three-glosses-wrap"), "en", "<p>", format!(
            "Read peace ({}) then war ({short_fill}) and also ({short_tail}) in this English sentence that keeps going on.",
            &short_tail[..short_tail.char_indices().nth(5).unwrap().0]
        )));
        out.push((format!("{script}/en/wrapped-gloss"), "en", "<p>", format!(
            "A long English sentence that runs on and on so that the gloss ({short_fill} {short_tail}) has to wrap across the end of a line somewhere here."
        )));
    }
    out
}

fn probe() {
    let styles = [
        "",
        "p { text-align: justify }",
        "p { text-align: justify; margin: 0 20% }",
        "p { text-align: justify; margin: 0 35% }",
        "p { margin: 0 25% }",
        "p { margin: 0 30% }",
        "p { margin: 0 40% }",
        "body { font-family: sans-serif } p { text-align: justify; margin: 0 15% }",
        "body { font-family: sans-serif } p { margin: 0 33% }",
    ];
    let mut rows = 0;
    let mut fails = 0;
    for (si, style) in styles.iter().enumerate() {
        for (name, lang, open, para) in paragraphs() {
            for (label, text) in [("pointed", para.clone()), ("twin", strip(&para))] {
                let body = format!("{open}{text}</p>");
                let bytes = styled_book(lang, style, &body);
                let doc = Document::open(bytes.clone()).expect("opens");
                let c = conservation(&bytes, &doc);
                let l = conservation_in_logical_order(&bytes, &doc);
                rows += 1;
                if !c.holds() || !l.holds() {
                    fails += 1;
                }
                emit!(
                    "ROW\ts{si}\t{name}\t{label}\tcontent={}\tlogical={}\t{:?}",
                    c.holds(),
                    l.holds(),
                    l.divergences.iter().take(1).collect::<Vec<_>>()
                );
            }
        }
    }
    emit!("rows={rows} fails={fails}");
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
