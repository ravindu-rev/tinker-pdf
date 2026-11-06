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
//!
//! With `--record BAR --revision REV RUN=REPORT...` it writes the bands: each
//! corpus's `peak` in `BAR` read from [`PEAK_RUNS`] dispatches' reports, and
//! the runs in the bar's `peak_runs`, as [`record`] says.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::json::{self, Json};
use crate::ratchet::{PeakBar, PEAK_RUNS, PEAK_TOLERANCE_PERCENT};

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
    read_json(&json::parse(text)?)
}

/// [`read`], of a report already parsed.
fn read_json(root: &Json) -> Result<Peaks, String> {
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

/// One dispatch's report, as `--record` reads it.
struct Dispatched {
    /// The workflow run's id.
    id: String,
    /// The machine it ran on, as its report states it.
    host: String,
    /// Each corpus's largest peak in it.
    peaks: Peaks,
}

/// A setting's value as a refusal states it.
fn shown(value: Option<&Json>) -> String {
    match value {
        None => "nothing".to_string(),
        Some(Json::String(text)) => format!("`{text}`"),
        Some(Json::Number(number)) => number.to_string(),
        Some(other) => other.to_pretty().trim_end().to_string(),
    }
}

/// `--record`: `bar`'s text with every corpus's `peak` read from the reports
/// of [`PEAK_RUNS`] dispatches at `revision`, `runs` each a run's id and its
/// report's text, and the runs and the revision in its `peak_runs`; with a
/// line per corpus saying what its band became.
///
/// A corpus's band is the largest of the runs' maxima, over the children they
/// agree reported one. Every refusal is named, and all are given at once: a
/// count of runs other than [`PEAK_RUNS`] or a run given twice; a report of
/// another schema, an incomplete one, one that names no host, or one run under
/// settings that are not the bar's; and per corpus, a run that lacks it or
/// none of whose children reported a peak, runs that disagree on its children,
/// fewer children than its committed band, and maxima that spread by
/// [`PEAK_TOLERANCE_PERCENT`] or more. A refused spread or denominator is the
/// owner's to judge (H-78), never banded wider.
pub fn record(
    bar: &str,
    revision: &str,
    runs: &[(String, String)],
) -> Result<(String, Vec<String>), String> {
    record_unheld(bar, revision, runs, &BTreeMap::new())
}

/// [`record`], with each corpus `unheld` names held to no band by the
/// owner's word (H-78): its maxima are read and said, and its `peak` is
/// written `{"unheld": "<reason>"}` in place of a band, whatever they spread
/// by. A name the bar has no corpus of is refused.
pub fn record_unheld(
    bar: &str,
    revision: &str,
    runs: &[(String, String)],
    unheld: &BTreeMap<String, String>,
) -> Result<(String, Vec<String>), String> {
    let committed = crate::ratchet::parse(bar).map_err(|e| format!("the bar: {e}"))?;
    let Json::Object(mut top) = json::parse(bar)? else {
        return Err("the bar is not an object".to_string());
    };
    if revision.trim().is_empty() {
        return Err("a band names the revision its runs ran at, and none was given".to_string());
    }
    if runs.len() != PEAK_RUNS {
        return Err(format!(
            "a band is read from {PEAK_RUNS} dispatches at one revision, and {} were given",
            runs.len()
        ));
    }
    let empty = BTreeMap::new();
    let settings = match top.get("settings") {
        Some(Json::Object(settings)) => settings,
        _ => &empty,
    };
    let mut refusals = Vec::new();
    let mut dispatched = Vec::new();
    for (index, (id, text)) in runs.iter().enumerate() {
        if runs[..index].iter().any(|(other, _)| other == id) {
            refusals.push(format!("run {id} is given twice"));
            continue;
        }
        let root = json::parse(text).map_err(|e| format!("run {id}: {e}"))?;
        let peaks = read_json(&root).map_err(|e| format!("run {id}: {e}"))?;
        if !matches!(root.get("complete"), Some(Json::Bool(true))) {
            refusals.push(format!(
                "run {id} is incomplete, and a band is read from complete runs"
            ));
        }
        let theirs = root.get("settings");
        for (key, value) in settings {
            let their = theirs.and_then(|settings| settings.get(key));
            if their != Some(value) {
                refusals.push(format!(
                    "run {id} ran with `{key}` {} and the bar is read under {}",
                    shown(their),
                    shown(Some(value))
                ));
            }
        }
        match theirs
            .and_then(|settings| settings.get("host"))
            .and_then(Json::as_str)
        {
            Some(host) => dispatched.push(Dispatched {
                id: id.clone(),
                host: host.to_string(),
                peaks,
            }),
            None => refusals.push(format!(
                "run {id} names no host, and a band names the machine each run landed on"
            )),
        }
    }

    for name in unheld.keys() {
        if committed.bars.iter().all(|corpus| &corpus.name != name) {
            refusals.push(format!(
                "`--unheld` names {name}, which the bar has no corpus of"
            ));
        }
    }
    let mut bands: BTreeMap<String, PeakBar> = BTreeMap::new();
    let mut lines = Vec::new();
    for corpus in &committed.bars {
        let name = &corpus.name;
        let mut maxima: Vec<(&str, &Largest)> = Vec::new();
        for run in &dispatched {
            match run.peaks.corpora.iter().find(|(other, _)| other == name) {
                None => refusals.push(format!("{name}: run {} has no such corpus", run.id)),
                Some((_, None)) => refusals.push(format!(
                    "{name}: no child of run {} reported a peak",
                    run.id
                )),
                Some((_, Some(largest))) => maxima.push((run.id.as_str(), largest)),
            }
        }
        if maxima.is_empty() || maxima.len() != dispatched.len() {
            continue;
        }
        if let Some(reason) = unheld.get(name) {
            let each: Vec<String> = maxima
                .iter()
                .map(|(id, largest)| format!("{} in run {id} ({})", largest.bytes, largest.file))
                .collect();
            lines.push(format!(
                "  {name}: held to no band by the owner's word ({reason}); maxima {}",
                each.join(", ")
            ));
            continue;
        }
        let children = maxima[0].1.children;
        if maxima
            .iter()
            .any(|(_, largest)| largest.children != children)
        {
            let each: Vec<String> = maxima
                .iter()
                .map(|(id, largest)| format!("{} in run {id}", largest.children))
                .collect();
            refusals.push(format!(
                "{name}: the runs' maxima are over different numbers of children, {}; \
                 a band is read over one denominator",
                each.join(", ")
            ));
            continue;
        }
        if let Some(before) = corpus.peak {
            if children < before.files {
                refusals.push(format!(
                    "{name}: the runs' maxima are over {children} children and its band over {}; \
                     a maximum over fewer children is not a smaller maximum",
                    before.files
                ));
                continue;
            }
        }
        let low = maxima
            .iter()
            .map(|(_, largest)| largest.bytes)
            .min()
            .unwrap_or(0);
        let high = maxima
            .iter()
            .map(|(_, largest)| largest.bytes)
            .max()
            .unwrap_or(0);
        if u128::from(high - low) * 100 >= u128::from(PEAK_TOLERANCE_PERCENT) * u128::from(low) {
            refusals.push(format!(
                "{name}: the runs' maxima spread from {low} to {high} bytes, {}, and a band is \
                 read from runs within {PEAK_TOLERANCE_PERCENT} % of each other",
                percent(high - low, low)
            ));
            continue;
        }
        let each: Vec<String> = maxima
            .iter()
            .map(|(id, largest)| format!("{} in run {id} ({})", largest.bytes, largest.file))
            .collect();
        let was = match corpus.peak {
            Some(before) => format!("{} bytes over {} children", before.bytes, before.files),
            None => "no band".to_string(),
        };
        lines.push(format!(
            "  {name}: band {high} bytes over {children} children, was {was}; maxima {}",
            each.join(", ")
        ));
        bands.insert(
            name.clone(),
            PeakBar {
                bytes: high,
                files: children,
            },
        );
    }
    let unbarred: BTreeSet<&String> = dispatched
        .iter()
        .flat_map(|run| run.peaks.corpora.iter().map(|(name, _)| name))
        .filter(|name| committed.bars.iter().all(|corpus| &corpus.name != *name))
        .collect();
    for name in unbarred {
        lines.push(format!(
            "  {name}: in the runs and not in the bar, so it has no band"
        ));
    }
    if !refusals.is_empty() {
        return Err(format!(
            "no band was recorded:\n  refused: {}",
            refusals.join("\n  refused: ")
        ));
    }

    if let Some(Json::Array(corpora)) = top.get_mut("corpora") {
        for corpus in corpora.iter_mut() {
            let Json::Object(fields) = corpus else {
                continue;
            };
            let name = fields
                .get("name")
                .and_then(Json::as_str)
                .map(str::to_string);
            if let Some(reason) = name.as_ref().and_then(|name| unheld.get(name)) {
                fields.insert(
                    "peak".to_string(),
                    Json::object([("unheld", Json::string(reason))]),
                );
                continue;
            }
            let band = name.as_ref().and_then(|name| bands.get(name)).copied();
            if let Some(band) = band {
                fields.insert(
                    "peak".to_string(),
                    Json::object([
                        ("bytes", Json::count(band.bytes)),
                        ("files", Json::count(band.files)),
                    ]),
                );
            }
        }
    }
    let dispatches = dispatched
        .iter()
        .map(|run| {
            Json::object([
                ("id", Json::string(&run.id)),
                ("host", Json::string(&run.host)),
            ])
        })
        .collect();
    top.insert(
        "peak_runs".to_string(),
        Json::object([
            ("revision", Json::string(revision)),
            ("dispatches", Json::Array(dispatches)),
        ]),
    );
    let text = Json::Object(top).to_pretty();
    crate::ratchet::parse(&text).map_err(|e| format!("the bar written does not read back: {e}"))?;
    Ok((text, lines))
}

/// `cargo xtask corpus-peaks --record BAR --revision REV
/// [--unheld CORPUS=REASON]... RUN=REPORT...`.
fn record_from(args: &[String]) -> Result<(), String> {
    let mut bar = None;
    let mut revision = None;
    let mut runs = Vec::new();
    let mut unheld = BTreeMap::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--record" => bar = Some(args.next().ok_or("`--record` takes the bar to write")?),
            "--unheld" => {
                let held = args
                    .next()
                    .ok_or("`--unheld` takes CORPUS=REASON, the owner's word")?;
                let (corpus, reason) = held
                    .split_once('=')
                    .filter(|(corpus, reason)| !corpus.is_empty() && !reason.trim().is_empty())
                    .ok_or_else(|| format!("`{held}` is not CORPUS=REASON"))?;
                unheld.insert(corpus.to_string(), reason.to_string());
            }
            "--revision" => {
                revision = Some(
                    args.next()
                        .ok_or("`--revision` takes the revision the runs ran at")?,
                );
            }
            flag if flag.starts_with("--") => return Err(format!("unknown option `{flag}`")),
            run => {
                let (id, path) = run
                    .split_once('=')
                    .ok_or_else(|| format!("`{run}` is not RUN-ID=REPORT"))?;
                let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
                runs.push((id.to_string(), text));
            }
        }
    }
    let bar = bar.ok_or("`--record` takes the bar to write")?;
    let revision = revision.ok_or("`--record` takes `--revision`, the revision the runs ran at")?;
    let text = std::fs::read_to_string(bar).map_err(|e| format!("{bar}: {e}"))?;
    let (written, lines) =
        record_unheld(&text, revision, &runs, &unheld).map_err(|e| format!("{bar}: {e}"))?;
    std::fs::write(bar, written).map_err(|e| format!("{bar}: {e}"))?;
    let ids: Vec<&str> = runs.iter().map(|(id, _)| id.as_str()).collect();
    println!(
        "corpus-peaks: recorded the peak bands of {bar} from runs {} at {revision}",
        ids.join(", ")
    );
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

/// `cargo xtask corpus-peaks <report>...`, or with `--record` the bands.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--record") {
        return record_from(args);
    }
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

    /// A bar `--record` reads bands into: one corpus, its band over `files`
    /// children.
    fn bar(files: u64) -> String {
        format!(
            r#"{{"schema": {}, "complete": true,
               "settings": {{"dpi": 72, "fonts": "none", "timeout_seconds": 180}},
               "corpora": [{{"name": "pdfjs", "total": 5, "passed": 5, "degraded": 0,
                 "strict_eligible": 5, "strict_clean": 5,
                 "peak": {{"bytes": 100, "files": {files}}}}}]}}"#,
            crate::report::SCHEMA
        )
    }

    /// One dispatch's report at `dpi`: a `pdfjs` child for each peak given.
    fn dispatch(dpi: u64, peaks: &[u64]) -> String {
        let files: Vec<String> = peaks
            .iter()
            .enumerate()
            .map(|(index, peak)| {
                format!(r#"{{"path": "{index}.pdf", "outcome": "passed", "peak": {peak}}}"#)
            })
            .collect();
        format!(
            r#"{{"schema": {}, "complete": true, "limits": [],
               "settings": {{"timeout_seconds": 180, "dpi": {dpi}, "fonts": "none",
                 "jobs": 4, "host": "Example CPU x 4"}},
               "corpora": [{{"name": "pdfjs", "files": [{}]}}]}}"#,
            crate::report::SCHEMA,
            files.join(", ")
        )
    }

    /// Three dispatches, runs 100, 101 and 102.
    fn three(reports: [String; 3]) -> Vec<(String, String)> {
        reports
            .into_iter()
            .enumerate()
            .map(|(index, report)| ((100 + index).to_string(), report))
            .collect()
    }

    #[test]
    fn the_band_is_the_largest_maximum_of_its_runs() {
        let (written, lines) = record(
            &bar(2),
            "0123abcd",
            &three([
                dispatch(72, &[90, 1000]),
                dispatch(72, &[1015, 80]),
                dispatch(72, &[1005, 70]),
            ]),
        )
        .expect("three runs within the tolerance");
        let read = crate::ratchet::parse(&written).expect("the bar reads back");
        assert_eq!(
            read.bars[0].peak,
            Some(PeakBar {
                bytes: 1015,
                files: 2
            })
        );
        let runs = read.peak_runs.expect("the runs it was read from");
        assert_eq!(runs.revision, "0123abcd");
        assert_eq!(
            runs.dispatches
                .iter()
                .map(|dispatch| (dispatch.id.as_str(), dispatch.host.as_str()))
                .collect::<Vec<_>>(),
            [
                ("100", "Example CPU x 4"),
                ("101", "Example CPU x 4"),
                ("102", "Example CPU x 4")
            ]
        );
        assert_eq!(
            lines,
            [
                "  pdfjs: band 1015 bytes over 2 children, was 100 bytes over 2 children; \
              maxima 1000 in run 100 (1.pdf), 1015 in run 101 (0.pdf), 1005 in run 102 (0.pdf)"
            ]
        );
    }

    #[test]
    fn runs_that_disagree_on_their_children_are_refused() {
        let refused = record(
            &bar(2),
            "0123abcd",
            &three([
                dispatch(72, &[1000, 90]),
                dispatch(72, &[1000, 90, 80]),
                dispatch(72, &[1000, 90]),
            ]),
        )
        .expect_err("a denominator the runs disagree on");
        assert!(
            refused.contains(
                "pdfjs: the runs' maxima are over different numbers of children, \
                 2 in run 100, 3 in run 101, 2 in run 102"
            ),
            "{refused}"
        );
    }

    #[test]
    fn fewer_children_than_the_bar_records_are_refused() {
        let refused = record(
            &bar(3),
            "0123abcd",
            &three([
                dispatch(72, &[1000, 90]),
                dispatch(72, &[1000, 90]),
                dispatch(72, &[1000, 90]),
            ]),
        )
        .expect_err("fewer children than the band");
        assert!(
            refused.contains("pdfjs: the runs' maxima are over 2 children and its band over 3"),
            "{refused}"
        );
    }

    /// **A corpus the owner holds to nothing is written with the reason**
    /// (H-78): a spread that would be refused is read and said, the corpus's
    /// `peak` names the reason in place of a band, and a name the bar has
    /// no corpus of is refused.
    #[test]
    fn an_unheld_corpus_is_written_with_its_reason_and_no_band() {
        let runs = three([
            dispatch(72, &[1000]),
            dispatch(72, &[1100]),
            dispatch(72, &[1000]),
        ]);
        let unheld = BTreeMap::from([("pdfjs".to_string(), "spread (MS-27)".to_string())]);
        let (written, lines) =
            record_unheld(&bar(1), "0123abcd", &runs, &unheld).expect("held to no band");
        let read = crate::ratchet::parse(&written).expect("the bar reads back");
        assert_eq!(read.bars[0].peak, None);
        assert_eq!(read.bars[0].peak_unheld.as_deref(), Some("spread (MS-27)"));
        assert_eq!(
            lines,
            [
                "  pdfjs: held to no band by the owner's word (spread (MS-27)); \
              maxima 1000 in run 100 (0.pdf), 1100 in run 101 (0.pdf), 1000 in run 102 (0.pdf)"
            ]
        );
        let elsewhere = BTreeMap::from([("qpdf".to_string(), "a reason".to_string())]);
        let refused =
            record_unheld(&bar(1), "0123abcd", &runs, &elsewhere).expect_err("no such corpus");
        assert!(
            refused.contains("`--unheld` names qpdf, which the bar has no corpus of"),
            "{refused}"
        );
    }

    #[test]
    fn a_spread_of_two_percent_is_refused_by_name() {
        let spread = |high: u64| {
            record(
                &bar(1),
                "0123abcd",
                &three([
                    dispatch(72, &[1000]),
                    dispatch(72, &[high]),
                    dispatch(72, &[1000]),
                ]),
            )
        };
        let refused = spread(1020).expect_err("a spread of two percent");
        assert!(
            refused.contains("pdfjs: the runs' maxima spread from 1000 to 1020 bytes, 2.0 %"),
            "{refused}"
        );
        assert!(spread(1019).is_ok(), "1.9 % is inside the tolerance");
    }

    #[test]
    fn runs_under_other_settings_are_refused() {
        let refused = record(
            &bar(1),
            "0123abcd",
            &three([
                dispatch(72, &[1000]),
                dispatch(150, &[1000]),
                dispatch(72, &[1000]),
            ]),
        )
        .expect_err("a run at another dpi");
        assert!(
            refused.contains("run 101 ran with `dpi` 150 and the bar is read under 72"),
            "{refused}"
        );
    }

    #[test]
    fn a_band_is_not_read_from_one_run() {
        let refused = record(
            &bar(1),
            "0123abcd",
            &[("100".to_string(), dispatch(72, &[1000]))],
        )
        .expect_err("one run");
        assert!(refused.contains("read from 3 dispatches"), "{refused}");
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
