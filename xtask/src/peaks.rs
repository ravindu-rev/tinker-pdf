//! `cargo xtask corpus-peaks <report>...`: each corpus's largest peak
//! resident set in each report, with where it was reached, beside the band
//! the committed bar holds it to (MS-19).
//!
//! It prints and decides nothing. A bar says a corpus's largest peak moved
//! past its band; the per-file report says which file reached it, how many
//! children reported a peak at all, in which phase it was reached and how much
//! of it was the binary's own pages. Given the reports of two revisions run
//! one after the other on one runner, the two lines per corpus say whether the
//! band was this runner's figure for either of them.

use std::collections::BTreeSet;
use std::path::Path;

use crate::json::{self, Json};

/// One corpus's largest peak in one report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Largest {
    /// The peak, in bytes.
    pub bytes: u64,
    /// The file whose child reached it, the first in the report's order
    /// where two did.
    pub file: String,
    /// How many children reported a peak at all: the maximum's denominator.
    pub children: u64,
    /// The first phase whose mark reached the peak, where the report carries
    /// the marks.
    pub phase: Option<String>,
    /// The file-backed part of that child's resident set, where the report
    /// carries it.
    pub file_backed: Option<u64>,
}

/// A report's `--fonts` setting and each corpus's largest peak, in the
/// report's order; `None` for a corpus no child reported one for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Peaks {
    /// The setting, which names the bar the bands come from.
    pub fonts: String,
    /// Per corpus.
    pub corpora: Vec<(String, Option<Largest>)>,
}

/// Reads a report `corpus-run --report` wrote, refusing one of another
/// schema: its fields may mean something else.
pub fn read(text: &str) -> Result<Peaks, String> {
    let root = json::parse(text)?;
    let schema = root
        .get("schema")
        .and_then(Json::as_u64)
        .ok_or("the report has no `schema`")?;
    if schema != crate::report::SCHEMA {
        return Err(format!(
            "the report is schema {schema} and this build reads schema {}",
            crate::report::SCHEMA
        ));
    }
    let fonts = root
        .get("settings")
        .and_then(|settings| settings.get("fonts"))
        .and_then(Json::as_str)
        .ok_or("the report does not say what `--fonts` it ran with")?
        .to_string();
    let mut corpora = Vec::new();
    for corpus in root
        .get("corpora")
        .and_then(Json::as_array)
        .ok_or("the report has no `corpora`")?
    {
        let name = corpus
            .get("name")
            .and_then(Json::as_str)
            .ok_or("a corpus in the report has no `name`")?;
        let mut largest: Option<Largest> = None;
        let mut children = 0u64;
        for file in corpus.get("files").and_then(Json::as_array).unwrap_or(&[]) {
            let Some(peak) = file.get("peak").and_then(Json::as_u64) else {
                continue;
            };
            children += 1;
            if largest.as_ref().is_some_and(|known| peak <= known.bytes) {
                continue;
            }
            let phase = file
                .get("hwm")
                .and_then(Json::as_array)
                .and_then(|marks| {
                    marks.iter().find(|mark| {
                        mark.get("bytes")
                            .and_then(Json::as_u64)
                            .is_some_and(|bytes| bytes >= peak)
                    })
                })
                .and_then(|mark| mark.get("phase"))
                .and_then(Json::as_str)
                .map(str::to_string);
            largest = Some(Largest {
                bytes: peak,
                file: file
                    .get("path")
                    .and_then(Json::as_str)
                    .unwrap_or("(no path)")
                    .to_string(),
                children: 0,
                phase,
                file_backed: file.get("rss_file").and_then(Json::as_u64),
            });
        }
        if let Some(known) = largest.as_mut() {
            known.children = children;
        }
        corpora.push((name.to_string(), largest));
    }
    Ok(Peaks { fonts, corpora })
}

/// A percentage of `of`, to one decimal place.
fn percent(part: u64, of: u64) -> String {
    if of == 0 {
        return "n/a".to_string();
    }
    format!("{:.1} %", part as f64 * 100.0 / of as f64)
}

/// The lines for one report, each corpus's peak beside `band`, the
/// committed band for that corpus where the bar has one.
pub fn lines(peaks: &Peaks, band: impl Fn(&str) -> Option<u64>) -> Vec<String> {
    let mut out = Vec::new();
    for (name, largest) in &peaks.corpora {
        let Some(largest) = largest else {
            out.push(format!("  {name}: no child reported a peak"));
            continue;
        };
        let mut line = format!(
            "  {name}: {} bytes, {}, over {} children",
            largest.bytes, largest.file, largest.children
        );
        if let Some(phase) = &largest.phase {
            line.push_str(&format!(", reached in {phase}"));
        }
        if let Some(file_backed) = largest.file_backed {
            line.push_str(&format!(
                ", {file_backed} bytes file-backed ({})",
                percent(file_backed, largest.bytes)
            ));
        }
        match band(name) {
            Some(band) => {
                let over = largest.bytes as f64 * 100.0 / band.max(1) as f64 - 100.0;
                line.push_str(&format!("; band {band} bytes, {over:+.1} %"));
            }
            None => line.push_str("; no committed band"),
        }
        out.push(line);
    }
    out
}

/// The corpora some report has and another lacks, a line each, naming the
/// reports that lack it.
pub fn unmatched(reports: &[(String, Peaks)]) -> Vec<String> {
    let names: BTreeSet<&String> = reports
        .iter()
        .flat_map(|(_, peaks)| peaks.corpora.iter().map(|(name, _)| name))
        .collect();
    let mut out = Vec::new();
    for name in names {
        let lacking: Vec<&str> = reports
            .iter()
            .filter(|(_, peaks)| !peaks.corpora.iter().any(|(other, _)| other == name))
            .map(|(path, _)| path.as_str())
            .collect();
        if !lacking.is_empty() {
            out.push(format!("corpus {name} is not in {}", lacking.join(", ")));
        }
    }
    out
}

/// `cargo xtask corpus-peaks <report>...`.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("corpus-peaks takes one report or more".to_string());
    }
    let mut reports = Vec::new();
    for path in args {
        if path.starts_with("--") {
            return Err(format!("unknown option `{path}`"));
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        reports.push((
            path.clone(),
            read(&text).map_err(|e| format!("{path}: {e}"))?,
        ));
    }
    for (path, peaks) in &reports {
        let bar_path = crate::corpus::ratchet_path(&peaks.fonts);
        let bar = std::fs::read_to_string(root.join(bar_path))
            .ok()
            .and_then(|text| crate::ratchet::parse(&text).ok());
        println!(
            "corpus-peaks: {path}, fonts {}, bands from {bar_path}",
            peaks.fonts
        );
        let band = |name: &str| {
            bar.as_ref()
                .and_then(|bar| bar.bars.iter().find(|b| b.name == name))
                .and_then(|b| b.peak.as_ref())
                .map(|peak| peak.bytes)
        };
        for line in lines(peaks, band) {
            println!("{line}");
        }
    }
    for line in unmatched(&reports) {
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(schema: u64, corpora: &str) -> String {
        format!(
            r#"{{"schema": {schema}, "complete": true, "limits": [],
               "settings": {{"timeout_seconds": 180, "dpi": 72, "fonts": "none", "jobs": 4}},
               "corpora": [{corpora}]}}"#
        )
    }

    const TWO: &str = r#"
        {"name": "pdfjs", "files": [
          {"path": "a.pdf", "outcome": "passed", "peak": 90},
          {"path": "b.pdf", "outcome": "passed", "peak": 100, "rss_file": 20,
           "hwm": [{"phase": "open", "bytes": 50}, {"phase": "render", "bytes": 100},
                   {"phase": "strict", "bytes": 100}]},
          {"path": "c.pdf", "outcome": "stalled"}
        ]},
        {"name": "qpdf", "files": [{"path": "d.pdf", "outcome": "passed", "peak": 5}]},
        {"name": "verapdf", "files": [{"path": "e.pdf", "outcome": "crashed"}]}"#;

    #[test]
    fn the_largest_peak_its_file_and_its_children_are_read_per_corpus() {
        let peaks = read(&report(crate::report::SCHEMA, TWO)).expect("a report");
        assert_eq!(peaks.fonts, "none");
        assert_eq!(
            peaks.corpora,
            [
                (
                    "pdfjs".to_string(),
                    Some(Largest {
                        bytes: 100,
                        file: "b.pdf".to_string(),
                        children: 2,
                        phase: Some("render".to_string()),
                        file_backed: Some(20),
                    })
                ),
                (
                    "qpdf".to_string(),
                    Some(Largest {
                        bytes: 5,
                        file: "d.pdf".to_string(),
                        children: 1,
                        phase: None,
                        file_backed: None,
                    })
                ),
                ("verapdf".to_string(), None),
            ]
        );
        let band = |name: &str| (name == "pdfjs").then_some(80);
        assert_eq!(
            lines(&peaks, band),
            [
                "  pdfjs: 100 bytes, b.pdf, over 2 children, reached in render, \
                 20 bytes file-backed (20.0 %); band 80 bytes, +25.0 %",
                "  qpdf: 5 bytes, d.pdf, over 1 children; no committed band",
                "  verapdf: no child reported a peak",
            ]
        );
    }

    #[test]
    fn a_report_of_another_schema_is_refused() {
        let refused = read(&report(crate::report::SCHEMA + 1, TWO)).expect_err("another schema");
        assert!(refused.contains("schema"), "{refused}");
    }

    #[test]
    fn a_corpus_one_report_lacks_is_named() {
        let both = read(&report(crate::report::SCHEMA, TWO)).expect("a report");
        let one = read(&report(
            crate::report::SCHEMA,
            r#"{"name": "pdfjs", "files": []}"#,
        ))
        .expect("a report");
        assert_eq!(
            unmatched(&[
                ("head.json".to_string(), both),
                ("bands.json".to_string(), one)
            ]),
            [
                "corpus qpdf is not in bands.json",
                "corpus verapdf is not in bands.json",
            ]
        );
    }
}
