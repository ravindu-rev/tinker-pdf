//! `zz_vx_probe_paren` (vx, round-5 review (confirm)): the books of
//! `tools/probes/rtl/probes/pdf/zz_vx_probe_paren.rs`, which is the probe as it ran.
//!
//! Probe: a bracketed run and its full stop in
//! right-to-left text, the shape CI's epub-corpus job reports at 20963 of
//! sample-regime-anticancer-arabic.epub (ROADMAP K-11). 240 books, each
//! pointed and as a mark-free twin.

use super::vov::styled_book;
use crate::epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::Document;

fn strip(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{064B}'..='\u{065F}' | '\u{0670}'))
        .collect()
}

fn probe() {
    let fillers = [
        "إنّ الخلايا السرطانيّة تتكاثر بسرعة كبيرة، ويعتمد ذلك على عوامل عدّة",
        "تتألّف كلّ الموادّ الحيّة على كوكبنا الأرض من خلايا، باستثناء الفيروسات",
    ];
    let insides = [
        "HDAC",
        "histone deacetylase",
        "النازعة للأسيتيل",
        "2010",
        "انظر الفصل الثالث",
    ];
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
    let dirs = ["<p dir=\"rtl\">", "<p>"];
    let mut rows = 0;
    let mut fails = 0;
    for style in styles {
        for dir in dirs {
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
                    for (label, text) in [("pointed", para.clone()), ("twin", strip(&para))] {
                        let body = format!("{dir}{text}</p>");
                        let bytes = styled_book("ar", style, &body);
                        let doc = Document::open(bytes.clone()).expect("opens");
                        let c = conservation(&bytes, &doc);
                        let l = conservation_in_logical_order(&bytes, &doc);
                        rows += 1;
                        if !c.holds() || !l.holds() {
                            fails += 1;
                            emit!(
                                "FAIL {label}\tstyle={style:?}\tdir={dir}\tinside={inside}\trepeat={repeat}\tcontent={}\tlogical={}\t{:?}",
                                c.holds(),
                                l.holds(),
                                l.divergences.iter().take(2).collect::<Vec<_>>()
                            );
                        }
                    }
                }
            }
        }
    }
    emit!("rows={rows} fails={fails}");
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
