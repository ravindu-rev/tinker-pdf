//! `zz_rr3m_hunt_pointed` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_pointed.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family pointed.

use super::common::{rep, run_book, strip_marks, Case, ARP, HEP};

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
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [
        ("hep", "he", HEP),
        ("arp", "ar", ARP),
        (
            "mix",
            "he",
            [
                HEP[0], ARP[0], HEP[1], ARP[1], HEP[2], ARP[2], HEP[3], ARP[3], HEP[4], ARP[4],
                HEP[5], ARP[5],
            ],
        ),
    ] {
        let s = |i: usize| w[i % w.len()];
        let mut structures: Vec<(String, String)> = Vec::new();
        let cells = |i: usize| -> String {
            [
                format!(
                    "<tr><td>{}</td><td>({})</td><td>{} {}.</td></tr>",
                    s(i),
                    s(i + 1),
                    s(i + 2),
                    s(i + 3)
                ),
                format!(
                    "<tr><td>«{}»</td><td>{}، {}</td><td>[{}]</td></tr>",
                    s(i + 4),
                    s(i + 5),
                    s(i + 6),
                    s(i + 7)
                ),
                format!(
                    "<tr><td>12</td><td>{} ١٢٣</td><td>{} ?!</td></tr>",
                    s(i + 8),
                    s(i + 9)
                ),
                format!(
                    "<tr><td>{} — {}</td><td>HDAC {}</td><td>{}:</td></tr>",
                    s(i + 10),
                    s(i + 11),
                    s(i),
                    s(i + 1)
                ),
            ]
            .concat()
        };
        for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
            structures.push((
                format!("table/{d}"),
                format!("<table{attr}>{}</table>", cells(0)),
            ));
            structures.push((
                format!("table-th/{d}"),
                format!(
                    "<table{attr}><tr><th>{}</th><th>({})</th></tr>{}</table>",
                    s(2),
                    s(3),
                    cells(3)
                ),
            ));
            structures.push((
                format!("table-cap/{d}"),
                format!(
                    "<table{attr}><caption>{} ({}) {}.</caption>{}</table>",
                    s(5),
                    s(6),
                    s(7),
                    cells(1)
                ),
            ));
            structures.push((format!("ul/{d}"), format!("<ul{attr}><li>{} <b>{}</b>.</li><li>({}) {}</li><li>«{}» {}?</li><li>{}: {}, {}</li></ul>", s(0), s(1), s(2), s(3), s(4), s(5), s(6), s(7), s(8))));
            structures.push((format!("ol-inside/{d}"), format!("<ol{attr} style=\"list-style-position: inside\"><li>{} ({})</li><li>[{}] {}.</li><li>{} ١٢ {}</li></ol>", s(1), s(2), s(3), s(4), s(5), s(6))));
            structures.push((
                format!("ol-long/{d}"),
                format!(
                    "<ol{attr}>{}</ol>",
                    (0..4)
                        .map(|k| format!(
                            "<li>{}</li>",
                            rep(2, |j| format!(
                                "{} {} ({}) {}، {}.",
                                s(k + j),
                                s(k + j + 1),
                                s(k + j + 2),
                                s(k + j + 3),
                                s(k + j + 4)
                            ))
                        ))
                        .collect::<String>()
                ),
            ));
            structures.push((
                format!("figcaption/{d}"),
                format!(
                    "<figure{attr}><figcaption>{} ({}) — «{} {}». {}</figcaption></figure>",
                    s(0),
                    s(1),
                    s(2),
                    s(3),
                    s(4)
                ),
            ));
            structures.push((
                format!("figcaption-long/{d}"),
                format!(
                    "<figure{attr}><figcaption>{}</figcaption></figure>",
                    rep(4, |j| format!(
                        "{} [{}] {}؛ {} «{}».",
                        s(j),
                        s(j + 1),
                        s(j + 2),
                        s(j + 3),
                        s(j + 4)
                    ))
                ),
            ));
            structures.push((
                format!("dl/{d}"),
                format!(
                    "<dl{attr}><dt>{}</dt><dd>{} ({}).</dd><dt>«{}»</dt><dd>{}!</dd></dl>",
                    s(0),
                    s(1),
                    s(2),
                    s(3),
                    s(4)
                ),
            ));
            structures.push((
                format!("h2p/{d}"),
                format!(
                    "<h2{attr}>{} ({})</h2><p{attr}>{}</p>",
                    s(0),
                    s(1),
                    rep(4, |j| format!(
                        "{} {} ({}) {}.",
                        s(j),
                        s(j + 1),
                        s(j + 2),
                        s(j + 3)
                    ))
                ),
            ));
            structures.push((
                format!("blockquote/{d}"),
                format!(
                    "<blockquote{attr}><p>«{} {}» — {}.</p></blockquote>",
                    s(0),
                    s(1),
                    s(2)
                ),
            ));
        }
        for (name, body) in structures {
            out.push((format!("{sc}/{name}/pointed"), lang, body.clone()));
            if sc != "mix" {
                out.push((format!("{sc}/{name}/twin"), lang, strip_marks(&body)));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "",
        "p, li, td, th, figcaption, dd, caption { text-align: justify } body { margin: 0 25% }",
        "td, th, li, figcaption, p { letter-spacing: 0.3em } table { border-spacing: 0 }",
        "td, th { padding: 0; letter-spacing: 0.55em } table { border-spacing: 1px }",
        "body { font-family: sans-serif; margin: 0 30% } p, li, td, figcaption { text-align: justify }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
