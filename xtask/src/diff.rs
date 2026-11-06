//! `cargo xtask corpus-diff BEFORE AFTER`: what changed, file by file,
//! between two per-file reports written under the same settings (MS-18).
//!
//! A bar holds counts and no names, so a relation that loses one file says
//! how many it lost and not which. Two reports of the same corpus name every
//! file and what its relations said, and the difference between them is the
//! lead: each file whose verdict differs, with what each report's child
//! measured, and per relation how many files went from held to another
//! verdict and how many came back. It reads two files and spawns nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::json::{self, Json};

/// What one relation said about one file, as a report records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// `held`, `broke` or `skipped`; `none` where the report asked nothing
    /// of the file, a child that did not finish among them.
    pub verdict: String,
    /// What the child measured, empty where it said nothing.
    pub detail: String,
}

impl Verdict {
    /// The word a file with no entry for a relation reads as.
    const NONE: &'static str = "none";

    fn none() -> Verdict {
        Verdict {
            verdict: Verdict::NONE.to_string(),
            detail: String::new(),
        }
    }

    fn held(&self) -> bool {
        self.verdict == "held"
    }

    fn shown(&self) -> String {
        if self.detail.is_empty() {
            self.verdict.clone()
        } else {
            format!("{} ({})", self.verdict, self.detail)
        }
    }
}

/// A report reduced to what a diff compares: its settings, and per corpus
/// each file's verdict on each relation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// The resolution, as the report writes it.
    pub dpi: String,
    /// The `--fonts` setting, as the report writes it.
    pub fonts: String,
    /// Corpus, then path, then relation.
    pub corpora: BTreeMap<String, BTreeMap<String, BTreeMap<String, Verdict>>>,
}

/// Reads a report `corpus-run --report` wrote.
pub fn parse(text: &str) -> Result<Report, String> {
    let root = json::parse(text)?;
    let settings = root.get("settings").ok_or("the report has no `settings`")?;
    let dpi = match settings.get("dpi") {
        Some(Json::Number(dpi)) => format!("{dpi}"),
        _ => return Err("the report's `settings` have no `dpi`".to_string()),
    };
    let fonts = settings
        .get("fonts")
        .and_then(Json::as_str)
        .ok_or("the report's `settings` have no `fonts`")?
        .to_string();
    let mut corpora = BTreeMap::new();
    for corpus in root
        .get("corpora")
        .and_then(Json::as_array)
        .ok_or("the report has no `corpora`")?
    {
        let name = corpus
            .get("name")
            .and_then(Json::as_str)
            .ok_or("a corpus in the report has no `name`")?;
        let mut files = BTreeMap::new();
        for file in corpus
            .get("files")
            .and_then(Json::as_array)
            .ok_or_else(|| format!("corpus `{name}` has no `files`"))?
        {
            let path = file
                .get("path")
                .and_then(Json::as_str)
                .ok_or_else(|| format!("a file of corpus `{name}` has no `path`"))?;
            let mut relations = BTreeMap::new();
            if let Some(Json::Object(entries)) = file.get("metamorphic") {
                for (relation, entry) in entries {
                    let verdict = entry
                        .get("verdict")
                        .and_then(Json::as_str)
                        .ok_or_else(|| format!("{name}/{path}: `{relation}` has no `verdict`"))?;
                    let detail = entry.get("detail").and_then(Json::as_str).unwrap_or("");
                    relations.insert(
                        relation.clone(),
                        Verdict {
                            verdict: verdict.to_string(),
                            detail: detail.to_string(),
                        },
                    );
                }
            }
            files.insert(path.to_string(), relations);
        }
        corpora.insert(name.to_string(), files);
    }
    Ok(Report {
        dpi,
        fonts,
        corpora,
    })
}

/// What moved between two reports of the same settings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    /// Per corpus and relation: files that went from held to another
    /// verdict, and files that came back to held.
    pub moves: BTreeMap<(String, String), (u64, u64)>,
    /// Every file whose verdict on a relation differs: corpus, relation,
    /// path, before, after.
    pub changed: Vec<(String, String, String, Verdict, Verdict)>,
    /// Files one report has and the other lacks: corpus, path, and whether
    /// it is the second report that has it.
    pub unmatched: Vec<(String, String, bool)>,
    /// Corpora only one report ran, with whether it is the second.
    pub corpora_unmatched: Vec<(String, bool)>,
}

/// Compares `after` with `before`, refusing two reports of different
/// settings: a verdict at another resolution, or with another face, is
/// another measurement, and a diff across them names files for the setting
/// rather than for the code.
pub fn diff(before: &Report, after: &Report) -> Result<Diff, String> {
    if before.dpi != after.dpi || before.fonts != after.fonts {
        return Err(format!(
            "the reports were written under different settings: dpi {} fonts {} against \
             dpi {} fonts {}, so a verdict that differs says which setting each ran in",
            before.dpi, before.fonts, after.dpi, after.fonts
        ));
    }
    let mut out = Diff::default();
    let names: BTreeSet<&String> = before.corpora.keys().chain(after.corpora.keys()).collect();
    for name in names {
        let (Some(was), Some(now)) = (before.corpora.get(name), after.corpora.get(name)) else {
            out.corpora_unmatched
                .push((name.clone(), after.corpora.contains_key(name)));
            continue;
        };
        let relations: BTreeSet<&String> = was
            .values()
            .chain(now.values())
            .flat_map(BTreeMap::keys)
            .collect();
        for relation in &relations {
            out.moves
                .insert((name.clone(), (*relation).clone()), (0, 0));
        }
        let paths: BTreeSet<&String> = was.keys().chain(now.keys()).collect();
        for path in paths {
            let (Some(was), Some(now)) = (was.get(path), now.get(path)) else {
                out.unmatched
                    .push((name.clone(), path.clone(), now.contains_key(path)));
                continue;
            };
            for relation in &relations {
                let before = was.get(*relation).cloned().unwrap_or_else(Verdict::none);
                let after = now.get(*relation).cloned().unwrap_or_else(Verdict::none);
                if before.verdict == after.verdict {
                    continue;
                }
                let tally = out
                    .moves
                    .entry((name.clone(), (*relation).clone()))
                    .or_default();
                if before.held() {
                    tally.0 += 1;
                } else if after.held() {
                    tally.1 += 1;
                }
                out.changed.push((
                    name.clone(),
                    (*relation).clone(),
                    path.clone(),
                    before,
                    after,
                ));
            }
        }
    }
    Ok(out)
}

impl Diff {
    /// The report, a line each.
    pub fn lines(&self, before: &str, after: &str) -> Vec<String> {
        let mut out = Vec::new();
        for ((corpus, relation), (from_held, back)) in &self.moves {
            out.push(format!(
                "{corpus} {relation}: {from_held} went from held, {back} came back to it"
            ));
            for (c, r, path, was, now) in &self.changed {
                if c == corpus && r == relation {
                    out.push(format!(
                        "  {corpus}/{path} {relation}: {} -> {}",
                        was.shown(),
                        now.shown()
                    ));
                }
            }
        }
        for (corpus, path, in_after) in &self.unmatched {
            out.push(format!(
                "only in {}: {corpus}/{path}, not compared",
                if *in_after { after } else { before }
            ));
        }
        for (corpus, in_after) in &self.corpora_unmatched {
            out.push(format!(
                "corpus {corpus} is only in {}, not compared",
                if *in_after { after } else { before }
            ));
        }
        out
    }

    /// The corpus and path of each file that went from held to broke on
    /// `relation`, for a workflow to print what each one is made of.
    pub fn held_to_broke(&self, relation: &str) -> Vec<(String, String)> {
        self.changed
            .iter()
            .filter(|(_, r, _, was, now)| r == relation && was.held() && now.verdict == "broke")
            .map(|(corpus, _, path, _, _)| (corpus.clone(), path.clone()))
            .collect()
    }
}

/// `cargo xtask corpus-diff BEFORE AFTER [--held-to-broke RELATION]`.
///
/// With `--held-to-broke`, only the files that went from held to broke on
/// that relation, a line each: the corpus, the path and the file on disk
/// under `root`'s lock, tab-separated, for a workflow to print what each one
/// is made of.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut paths = Vec::new();
    let mut listing: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--held-to-broke" => {
                index += 1;
                listing = Some(
                    args.get(index)
                        .cloned()
                        .ok_or("`--held-to-broke` needs a relation")?,
                );
            }
            other if other.starts_with("--") => return Err(format!("unknown option `{other}`")),
            path => paths.push(path.to_string()),
        }
        index += 1;
    }
    let [before_path, after_path] = paths.as_slice() else {
        return Err("corpus-diff takes two reports, BEFORE and AFTER".to_string());
    };
    let read = |path: &str| -> Result<Report, String> {
        let text = std::fs::read_to_string(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
        parse(&text).map_err(|e| format!("{path}: {e}"))
    };
    let (before, after) = (read(before_path)?, read(after_path)?);
    let found = diff(&before, &after)?;
    if let Some(relation) = listing {
        let corpora = crate::lock::read(root)?;
        for (corpus, path) in found.held_to_broke(&relation) {
            let file = corpora
                .iter()
                .find(|locked| locked.name == corpus)
                .map(|locked| crate::lock::files_dir(root, locked).join(&path))
                .ok_or_else(|| {
                    format!("`{corpus}` is not a corpus in {}", crate::lock::LOCK_PATH)
                })?;
            println!("{corpus}\t{path}\t{}", file.display());
        }
        return Ok(());
    }
    println!(
        "corpus-diff: {before_path} against {after_path}, dpi {}, fonts {}",
        before.dpi, before.fonts
    );
    for line in found.lines(before_path, after_path) {
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file and its relations' verdicts: `(path, [(relation, verdict, detail)])`.
    type File<'a> = (&'a str, &'a [(&'a str, &'a str, &'a str)]);

    /// A report of one corpus at 72 dpi, its files given as [`File`]s.
    fn report(fonts: &str, corpus: &str, files: &[File<'_>]) -> Report {
        let mut out = Report {
            dpi: "72".to_string(),
            fonts: fonts.to_string(),
            corpora: BTreeMap::new(),
        };
        let mut paths = BTreeMap::new();
        for (path, relations) in files {
            let mut verdicts = BTreeMap::new();
            for (relation, verdict, detail) in *relations {
                verdicts.insert(
                    (*relation).to_string(),
                    Verdict {
                        verdict: (*verdict).to_string(),
                        detail: (*detail).to_string(),
                    },
                );
            }
            paths.insert((*path).to_string(), verdicts);
        }
        out.corpora.insert(corpus.to_string(), paths);
        out
    }

    #[test]
    fn a_verdict_that_moved_is_named_with_both_details() {
        let before = report(
            "none",
            "pdfjs",
            &[("a.pdf", &[("dpi", "held", "12 of 9600")])],
        );
        let after = report(
            "none",
            "pdfjs",
            &[("a.pdf", &[("dpi", "broke", "230 of 9600")])],
        );
        let found = diff(&before, &after).expect("same settings");
        let lines = found.lines("BEFORE", "AFTER");
        assert_eq!(
            lines,
            [
                "pdfjs dpi: 1 went from held, 0 came back to it",
                "  pdfjs/a.pdf dpi: held (12 of 9600) -> broke (230 of 9600)",
            ]
        );
        assert_eq!(
            found.held_to_broke("dpi"),
            [("pdfjs".to_string(), "a.pdf".to_string())]
        );
        assert!(found.held_to_broke("rotate").is_empty());
    }

    #[test]
    fn each_relation_counts_its_moves_both_ways() {
        let before = report(
            "none",
            "pdfjs",
            &[
                ("a.pdf", &[("dpi", "held", ""), ("rotate", "broke", "")]),
                ("b.pdf", &[("dpi", "held", ""), ("rotate", "held", "")]),
                (
                    "c.pdf",
                    &[("dpi", "skipped", "too large"), ("rotate", "held", "")],
                ),
            ],
        );
        let after = report(
            "none",
            "pdfjs",
            &[
                ("a.pdf", &[("dpi", "broke", ""), ("rotate", "held", "")]),
                (
                    "b.pdf",
                    &[("dpi", "skipped", "too large"), ("rotate", "held", "")],
                ),
                ("c.pdf", &[("dpi", "held", ""), ("rotate", "held", "")]),
            ],
        );
        let found = diff(&before, &after).expect("same settings");
        assert_eq!(
            found.moves[&("pdfjs".to_string(), "dpi".to_string())],
            (2, 1)
        );
        assert_eq!(
            found.moves[&("pdfjs".to_string(), "rotate".to_string())],
            (0, 1)
        );
        assert_eq!(found.changed.len(), 4);
        // Only a held file that broke is listed for the objects and probe.
        assert_eq!(
            found.held_to_broke("dpi"),
            [("pdfjs".to_string(), "a.pdf".to_string())]
        );
    }

    #[test]
    fn a_file_one_report_lacks_is_named_and_not_compared() {
        let before = report("none", "pdfjs", &[("a.pdf", &[("dpi", "held", "")])]);
        let mut after = report(
            "none",
            "pdfjs",
            &[
                ("a.pdf", &[("dpi", "held", "")]),
                ("b.pdf", &[("dpi", "broke", "")]),
            ],
        );
        after.corpora.insert("qpdf".to_string(), BTreeMap::new());
        let found = diff(&before, &after).expect("same settings");
        assert!(found.changed.is_empty(), "{:?}", found.changed);
        assert_eq!(
            found.moves[&("pdfjs".to_string(), "dpi".to_string())],
            (0, 0)
        );
        assert_eq!(
            found.lines("BEFORE", "AFTER"),
            [
                "pdfjs dpi: 0 went from held, 0 came back to it",
                "only in AFTER: pdfjs/b.pdf, not compared",
                "corpus qpdf is only in AFTER, not compared",
            ]
        );
    }

    #[test]
    fn reports_of_two_settings_are_refused() {
        let none = report("none", "pdfjs", &[("a.pdf", &[("dpi", "held", "")])]);
        let faced = report("synthetic", "pdfjs", &[("a.pdf", &[("dpi", "broke", "")])]);
        let refused = diff(&none, &faced).expect_err("two settings");
        assert!(refused.contains("different settings"), "{refused}");
        let mut doubled = none.clone();
        doubled.dpi = "144".to_string();
        assert!(diff(&none, &doubled).is_err());
    }

    #[test]
    fn a_report_reads_back_as_corpus_run_writes_it() {
        let text = r#"{
          "schema": 2, "complete": true, "limits": [],
          "settings": {"timeout_seconds": 180, "dpi": 72, "fonts": "none", "jobs": 4},
          "corpora": [{"name": "pdfjs", "files": [
            {"path": "a.pdf", "outcome": "passed",
             "metamorphic": {"dpi": {"verdict": "held", "detail": "12 of 9600"},
                             "rotate": {"verdict": "skipped"}}},
            {"path": "b.pdf", "outcome": "crashed"}
          ], "summary": {}}]
        }"#;
        let read = parse(text).expect("a report");
        assert_eq!(read.fonts, "none");
        let files = &read.corpora["pdfjs"];
        assert_eq!(files["a.pdf"]["dpi"].detail, "12 of 9600");
        assert_eq!(files["a.pdf"]["rotate"].verdict, "skipped");
        assert!(files["b.pdf"].is_empty(), "a crash asked nothing");
    }
}
