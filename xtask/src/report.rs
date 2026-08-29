//! What a run produces, and how it says what it did not do.
//!
//! Two documents come out of a run and they are not the same thing:
//!
//! - the **report**, one entry per file, written to a path the caller names
//!   and never committed. It is what somebody reads when a number moves.
//! - the **ratchet**, the counts alone, committed. It is what CI compares.
//!
//! Both carry the same `limits` list, and it is the most important field in
//! either. A run that sampled, or stopped early, or skipped a corpus it could
//! not find, says so *in the document* — not in the console output that
//! scrolled past. Plan 23: "a truncated run that reads like a complete one is
//! how a corpus stops meaning anything." The ratchet comparator refuses to
//! ratchet against a run whose limits are not empty, so the rule is enforced
//! rather than merely documented.

use std::collections::BTreeMap;

use crate::json::Json;
use crate::runner::FileResult;

/// The schema version of both documents.
pub const SCHEMA: u64 = 2;

/// One corpus's results.
#[derive(Clone, Debug, Default)]
pub struct CorpusReport {
    /// The lock's name for it.
    pub name: String,
    /// Every file that was run.
    pub files: Vec<FileResult>,
}

impl CorpusReport {
    /// How many files were run.
    pub fn total(&self) -> u64 {
        self.files.len() as u64
    }

    /// How many returned a bitmap for every page without crashing or timing
    /// out. Ruling 2's definition; see [`crate::runner::Outcome::Passed`].
    pub fn passed(&self) -> u64 {
        self.files
            .iter()
            .filter(|f| matches!(f.outcome, crate::runner::Outcome::Passed))
            .count() as u64
    }

    /// How many reported anything at all while doing it.
    ///
    /// The second axis, and it is a *share of what was run* rather than a
    /// share of what passed: a file that crashed reported plenty and is not
    /// counted here, because the question this answers is "of the files that
    /// came back, how many came back clean".
    pub fn degraded(&self) -> u64 {
        self.files.iter().filter(|f| f.degraded()).count() as u64
    }

    /// How many files each outcome accounts for.
    pub fn outcomes(&self) -> BTreeMap<&'static str, u64> {
        let mut out = BTreeMap::new();
        for label in ["passed", "failed", "crashed", "timed_out"] {
            out.insert(label, 0);
        }
        for file in &self.files {
            *out.entry(file.outcome.label()).or_default() += 1;
        }
        out
    }

    /// How many *files* need each capability.
    ///
    /// Files rather than occurrences: gaps 17 and 18 are deciding whether to
    /// build a decoder, and "two hundred files cannot render" is the number
    /// that decides it. One file with four hundred JBIG2 images is one file.
    pub fn capabilities(&self) -> BTreeMap<String, u64> {
        let mut out = BTreeMap::new();
        for file in &self.files {
            for capability in &file.capabilities {
                *out.entry(capability.clone()).or_default() += 1;
            }
        }
        out
    }

    /// How many files the strict pass ran on (ruling 13).
    ///
    /// A file is eligible when this engine read it cleanly and could rewrite
    /// it, which is what makes the rate below a statement about the *writer*
    /// rather than about the corpus.
    pub fn strict_eligible(&self) -> u64 {
        self.files
            .iter()
            .filter(|file| file.strict.eligible())
            .count() as u64
    }

    /// How many of those rewrites carried no structural defect at all.
    pub fn strict_clean(&self) -> u64 {
        self.files.iter().filter(|file| file.strict.clean()).count() as u64
    }

    /// How many files each strict defect kind was found in.
    ///
    /// Files rather than occurrences, for the reason `capabilities` counts
    /// files: "eleven documents have a backwards `/Rect`" is the number that
    /// decides whether a rule is worth acting on.
    pub fn strict_kinds(&self) -> BTreeMap<String, u64> {
        let mut out = BTreeMap::new();
        for file in &self.files {
            if let crate::runner::Strict::Checked { kinds, .. } = &file.strict {
                for label in kinds.keys() {
                    *out.entry(label.clone()).or_default() += 1;
                }
            }
        }
        out
    }

    /// What the structure tree walk reached over this corpus (14.7).
    ///
    /// Summed over files rather than averaged over them: a per-file rate
    /// averaged is not the rate over the corpus, and most corpus files carry
    /// no structure tree at all.
    pub fn tagged(&self) -> crate::ratchet::TaggedBar {
        let mut out = crate::ratchet::TaggedBar::default();
        for file in &self.files {
            let Some(tagged) = file.tagged else {
                continue;
            };
            out.files += 1;
            out.elements += tagged.elements;
            out.matched += tagged.matched;
            out.orphans += tagged.orphans;
        }
        out
    }

    /// How many files each metamorphic relation was **asked** of, by name.
    ///
    /// Asked and held are two counts and both are recorded, for the reason the
    /// strict pass records eligible beside clean: a relation that declines the
    /// hard files and holds on the rest is not a relation that held.
    pub fn metamorphic_compared(&self) -> BTreeMap<String, u64> {
        self.metamorphic(|verdict| verdict.compared())
    }

    /// How many files each relation held on.
    pub fn metamorphic_held(&self) -> BTreeMap<String, u64> {
        self.metamorphic(|verdict| verdict.held())
    }

    fn metamorphic(
        &self,
        wanted: fn(&crate::runner::MetaVerdict) -> bool,
    ) -> BTreeMap<String, u64> {
        let mut out = BTreeMap::new();
        for file in &self.files {
            for (name, verdict) in &file.metamorphic {
                let slot = out.entry(name.clone()).or_default();
                if wanted(verdict) {
                    *slot += 1;
                }
            }
        }
        out
    }

    /// How many files reported each warning label, most common first when
    /// rendered.
    pub fn warnings(&self) -> BTreeMap<String, u64> {
        let mut out = BTreeMap::new();
        for file in &self.files {
            for label in file.warnings.keys() {
                *out.entry(label.clone()).or_default() += 1;
            }
        }
        out
    }
}

/// One file's strict verdict, as the report carries it.
fn strict_json(strict: &crate::runner::Strict) -> Json {
    match strict {
        crate::runner::Strict::Ineligible(reason) => Json::object([
            ("eligible", Json::Bool(false)),
            ("reason", Json::string(reason)),
        ]),
        crate::runner::Strict::Checked {
            structure,
            semantics,
            kinds,
        } => Json::object([
            ("eligible", Json::Bool(true)),
            ("structure", Json::count(*structure)),
            ("semantics", Json::count(*semantics)),
            (
                "kinds",
                Json::object(
                    kinds
                        .iter()
                        .map(|(k, v)| (k.clone(), Json::count(*v as u64))),
                ),
            ),
        ]),
    }
}

/// How the run was configured, recorded so a number can be read years later.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The per-file timeout in seconds.
    pub timeout_seconds: u64,
    /// The resolution every page was rendered at.
    pub dpi: f64,
    /// What `--fonts` was given, or `none`.
    ///
    /// Never omitted and never blank. This engine bundles no faces, so a
    /// warning count taken without a provider is dominated by documents that
    /// embed nothing — a fact about the build's font policy rather than about
    /// the engine — and a number recorded without saying which it was is a
    /// number nobody can use.
    pub fonts: String,
}

/// A whole run.
#[derive(Clone, Debug)]
pub struct Run {
    /// Per corpus, in lock order.
    pub corpora: Vec<CorpusReport>,
    /// Everything this run did not do: samples, early stops, corpora that
    /// were not on disk. Empty means the run was complete.
    pub limits: Vec<String>,
    /// How it was configured.
    pub settings: Settings,
}

impl Run {
    /// Whether the run covered everything it was asked to.
    pub fn complete(&self) -> bool {
        self.limits.is_empty()
    }

    pub fn total(&self) -> u64 {
        self.corpora.iter().map(CorpusReport::total).sum()
    }

    pub fn passed(&self) -> u64 {
        self.corpora.iter().map(CorpusReport::passed).sum()
    }

    /// The full document, one entry per file.
    pub fn to_report_json(&self) -> Json {
        let corpora = self
            .corpora
            .iter()
            .map(|corpus| {
                let files = corpus
                    .files
                    .iter()
                    .map(|file| {
                        let mut fields = vec![
                            ("path", Json::string(&file.path)),
                            ("outcome", Json::string(file.outcome.label())),
                            ("pages", Json::count(u64::from(file.pages))),
                            ("rendered", Json::count(u64::from(file.rendered))),
                            ("millis", Json::count(file.millis)),
                        ];
                        if let crate::runner::Outcome::Failed(reason)
                        | crate::runner::Outcome::Crashed(reason) = &file.outcome
                        {
                            fields.push(("reason", Json::string(reason)));
                        }
                        // Where a killed child had got to. Written for both
                        // states rather than only for the stalled one: "timed
                        // out at page 340 of 900" and "stalled at strict" are
                        // the two sentences that save whoever reads this from
                        // reproducing the run before they can start on it.
                        if let crate::runner::Outcome::TimedOut { at }
                        | crate::runner::Outcome::Stalled { at } = &file.outcome
                        {
                            if !at.is_empty() {
                                fields.push(("at", Json::string(at)));
                            }
                        }
                        if !file.capabilities.is_empty() {
                            fields.push((
                                "capabilities",
                                Json::Array(file.capabilities.iter().map(Json::string).collect()),
                            ));
                        }
                        if !file.warnings.is_empty() {
                            fields.push((
                                "warnings",
                                Json::object(
                                    file.warnings
                                        .iter()
                                        .map(|(k, v)| (k.clone(), Json::count(*v as u64))),
                                ),
                            ));
                        }
                        if file.cost != crate::runner::Cost::default() {
                            fields.push((
                                "cost",
                                Json::object([
                                    ("bytes", Json::count(file.cost.bytes)),
                                    ("objects", Json::count(file.cost.objects)),
                                    ("pixels", Json::count(file.cost.pixels)),
                                ]),
                            ));
                        }
                        fields.push(("strict", strict_json(&file.strict)));
                        // The fourth axis, per file. Without it the ratchet
                        // can say `dpi held on 572 of 580, worse than the
                        // recorded 572 of 579` and nothing in the run says
                        // *which* file — which makes the number a verdict
                        // rather than a lead, and `corpus.yml` keeps this
                        // report precisely so a moved bar can be followed up.
                        if !file.metamorphic.is_empty() {
                            fields.push((
                                "metamorphic",
                                Json::object(file.metamorphic.iter().map(|(name, verdict)| {
                                    let mut row = vec![("verdict", Json::string(verdict.label()))];
                                    if !verdict.detail().is_empty() {
                                        row.push(("detail", Json::string(verdict.detail())));
                                    }
                                    (name.clone(), Json::object(row))
                                })),
                            ));
                        }
                        Json::object(fields)
                    })
                    .collect();
                Json::object([
                    ("name", Json::string(&corpus.name)),
                    ("files", Json::Array(files)),
                    ("summary", self.corpus_summary(corpus)),
                ])
            })
            .collect();

        Json::object([
            ("schema", Json::count(SCHEMA)),
            ("complete", Json::Bool(self.complete())),
            (
                "limits",
                Json::Array(self.limits.iter().map(Json::string).collect()),
            ),
            ("settings", self.settings_json()),
            ("corpora", Json::Array(corpora)),
        ])
    }

    fn settings_json(&self) -> Json {
        Json::object([
            (
                "timeout_seconds",
                Json::count(self.settings.timeout_seconds),
            ),
            ("dpi", Json::Number(self.settings.dpi)),
            ("fonts", Json::string(&self.settings.fonts)),
        ])
    }

    fn corpus_summary(&self, corpus: &CorpusReport) -> Json {
        Json::object([
            ("total", Json::count(corpus.total())),
            ("passed", Json::count(corpus.passed())),
            ("degraded", Json::count(corpus.degraded())),
            ("strict_eligible", Json::count(corpus.strict_eligible())),
            ("strict_clean", Json::count(corpus.strict_clean())),
            (
                "strict_kinds",
                Json::object(
                    corpus
                        .strict_kinds()
                        .into_iter()
                        .map(|(k, v)| (k, Json::count(v))),
                ),
            ),
            (
                "outcomes",
                Json::object(
                    corpus
                        .outcomes()
                        .into_iter()
                        .map(|(k, v)| (k.to_string(), Json::count(v))),
                ),
            ),
            (
                "capabilities",
                Json::object(
                    corpus
                        .capabilities()
                        .into_iter()
                        .map(|(k, v)| (k, Json::count(v))),
                ),
            ),
            (
                "warnings",
                Json::object(
                    corpus
                        .warnings()
                        .into_iter()
                        .map(|(k, v)| (k, Json::count(v))),
                ),
            ),
        ])
    }

    /// The counts alone: what gets committed and compared.
    pub fn to_ratchet_json(&self, note: &str) -> Json {
        let corpora = self
            .corpora
            .iter()
            .map(|corpus| {
                Json::object([
                    ("name", Json::string(&corpus.name)),
                    ("total", Json::count(corpus.total())),
                    ("passed", Json::count(corpus.passed())),
                    ("degraded", Json::count(corpus.degraded())),
                    ("strict_eligible", Json::count(corpus.strict_eligible())),
                    ("strict_clean", Json::count(corpus.strict_clean())),
                    (
                        "outcomes",
                        Json::object(
                            corpus
                                .outcomes()
                                .into_iter()
                                .map(|(k, v)| (k.to_string(), Json::count(v))),
                        ),
                    ),
                    (
                        "capabilities",
                        Json::object(
                            corpus
                                .capabilities()
                                .into_iter()
                                .map(|(k, v)| (k, Json::count(v))),
                        ),
                    ),
                    // Roadmap step 7. Counts and never rates, and **both**
                    // counts: a relation's held figure means nothing without
                    // the number of files it was asked of.
                    (
                        "metamorphic",
                        Json::object(corpus.metamorphic_compared().into_iter().map(
                            |(name, compared)| {
                                let held =
                                    corpus.metamorphic_held().get(&name).copied().unwrap_or(0);
                                (
                                    name,
                                    Json::object([
                                        ("compared", Json::count(compared)),
                                        ("held", Json::count(held)),
                                    ]),
                                )
                            },
                        )),
                    ),
                    // Tagged PDF milestone 4. Four counts, and the reason
                    // `orphans` is recorded beside `matched` rather than on
                    // its own is that a tree claiming fewer characters and a
                    // corpus offering fewer marked characters are different
                    // facts that a lone orphan count cannot tell apart.
                    ("tagged", {
                        let tagged = corpus.tagged();
                        Json::object([
                            ("files", Json::count(tagged.files)),
                            ("elements", Json::count(tagged.elements)),
                            ("matched", Json::count(tagged.matched)),
                            ("orphans", Json::count(tagged.orphans)),
                        ])
                    }),
                ])
            })
            .collect();

        Json::object([
            ("schema", Json::count(SCHEMA)),
            ("note", Json::string(note)),
            ("complete", Json::Bool(self.complete())),
            (
                "limits",
                Json::Array(self.limits.iter().map(Json::string).collect()),
            ),
            ("settings", self.settings_json()),
            ("corpora", Json::Array(corpora)),
        ])
    }

    /// The lines a person pastes into the gap doc when a number improves.
    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for corpus in &self.corpora {
            let outcomes = corpus.outcomes();
            lines.push(format!(
                "{:<14} {:>6} files  {:>6} passed  {:>6} degraded  \
                 (failed {}, crashed {}, timed out {}, stalled {})",
                corpus.name,
                corpus.total(),
                corpus.passed(),
                corpus.degraded(),
                outcomes.get("failed").copied().unwrap_or(0),
                outcomes.get("crashed").copied().unwrap_or(0),
                outcomes.get("timed_out").copied().unwrap_or(0),
                // Its own column, not folded into the one before it. A stalled
                // file is a defect in this engine and a timed-out one is a
                // large document; a single number for both is what let a
                // non-terminating rewrite sit in the corpus unnoticed.
                outcomes.get("stalled").copied().unwrap_or(0),
            ));
        }
        lines.push(format!(
            "{:<14} {:>6} files  {:>6} passed",
            "all",
            self.total(),
            self.passed()
        ));

        // Ruling 13's axis, printed as its own line rather than folded into
        // the one above: it answers a different question — of the files this
        // engine read cleanly, how many produced a rewrite that holds up to
        // ISO 32000 read strictly — and a job that greps for it can tell that
        // the pass ran at all.
        let eligible: u64 = self.corpora.iter().map(CorpusReport::strict_eligible).sum();
        let clean: u64 = self.corpora.iter().map(CorpusReport::strict_clean).sum();
        lines.push(format!(
            "{:<14} {:>6} rewritten  {:>6} validate strictly",
            "strict", eligible, clean
        ));
        if !self.complete() {
            lines.push(String::from(
                "INCOMPLETE — this run does not describe the whole corpus:",
            ));
            for limit in &self.limits {
                lines.push(format!("  - {limit}"));
            }
        }
        lines
    }

    /// The capability hit-rate table, as markdown, for gaps 10, 17 and 18.
    pub fn capability_table(&self) -> String {
        let mut names: Vec<String> = Vec::new();
        for corpus in &self.corpora {
            for name in corpus.capabilities().keys() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        names.sort();

        let mut out = String::from("| Corpus | Files |");
        for name in &names {
            out.push_str(&format!(" {name} |"));
        }
        out.push_str("\n| --- | ---: |");
        for _ in &names {
            out.push_str(" ---: |");
        }
        out.push('\n');

        let mut totals: BTreeMap<String, u64> = BTreeMap::new();
        let mut all = 0u64;
        for corpus in &self.corpora {
            let hits = corpus.capabilities();
            all += corpus.total();
            out.push_str(&format!("| `{}` | {} |", corpus.name, corpus.total()));
            for name in &names {
                let count = hits.get(name).copied().unwrap_or(0);
                *totals.entry(name.clone()).or_default() += count;
                out.push_str(&format!(" {count} |"));
            }
            out.push('\n');
        }
        out.push_str(&format!("| **all** | **{all}** |"));
        for name in &names {
            let count = totals.get(name).copied().unwrap_or(0);
            // The share is what the decision turns on, and it is computed
            // here for reading rather than for comparing: nothing ratchets
            // against a percentage.
            let share = if all == 0 {
                0.0
            } else {
                (count as f64) * 100.0 / (all as f64)
            };
            out.push_str(&format!(" **{count}** ({share:.1}%) |"));
        }
        out.push('\n');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::Outcome;
    use std::collections::{BTreeMap, BTreeSet};

    fn file(path: &str, outcome: Outcome, warnings: &[&str], capabilities: &[&str]) -> FileResult {
        FileResult {
            path: path.to_string(),
            outcome,
            cost: crate::runner::Cost::default(),
            bundled_faces: false,
            tagged: None,
            pages: 1,
            rendered: 1,
            warnings: warnings
                .iter()
                .map(|w| ((*w).to_string(), 1usize))
                .collect::<BTreeMap<_, _>>(),
            capabilities: capabilities
                .iter()
                .map(|c| (*c).to_string())
                .collect::<BTreeSet<_>>(),
            millis: 1,
            metamorphic: BTreeMap::new(),
            strict: crate::runner::Strict::Checked {
                structure: 0,
                semantics: 0,
                kinds: BTreeMap::new(),
            },
        }
    }

    fn run() -> Run {
        Run {
            corpora: vec![CorpusReport {
                name: "synthetic".to_string(),
                files: vec![
                    file("a.pdf", Outcome::Passed, &[], &[]),
                    file(
                        "b.pdf",
                        Outcome::Passed,
                        &["render:UnreadableFont"],
                        &["jbig2"],
                    ),
                    file("c.pdf", Outcome::Failed("no".into()), &[], &["jpx"]),
                    file("d.pdf", Outcome::Crashed("boom".into()), &[], &[]),
                    file(
                        "e.pdf",
                        Outcome::TimedOut {
                            at: "page 3/900".into(),
                        },
                        &[],
                        &[],
                    ),
                ],
            }],
            limits: Vec::new(),
            settings: Settings {
                timeout_seconds: 20,
                dpi: 72.0,
                fonts: "none".to_string(),
            },
        }
    }

    #[test]
    fn the_summary_counts_every_outcome() {
        let run = run();
        let corpus = &run.corpora[0];
        assert_eq!(corpus.total(), 5);
        assert_eq!(corpus.passed(), 2);
        assert_eq!(corpus.degraded(), 1);
        let outcomes = corpus.outcomes();
        assert_eq!(outcomes["passed"], 2);
        assert_eq!(outcomes["failed"], 1);
        assert_eq!(outcomes["crashed"], 1);
        assert_eq!(outcomes["timed_out"], 1);
    }

    /// Every outcome appears even at zero. A key that vanished when nothing
    /// timed out would make "no timeouts" and "timeouts not measured" look
    /// the same in the committed file.
    #[test]
    fn an_outcome_that_did_not_happen_is_still_reported_as_zero() {
        let run = Run {
            corpora: vec![CorpusReport {
                name: "clean".to_string(),
                files: vec![file("a.pdf", Outcome::Passed, &[], &[])],
            }],
            ..run()
        };
        let outcomes = run.corpora[0].outcomes();
        assert_eq!(outcomes["timed_out"], 0);
        assert_eq!(outcomes["crashed"], 0);
    }

    #[test]
    fn a_capability_is_counted_once_per_file() {
        let hits = run().corpora[0].capabilities();
        assert_eq!(hits["jbig2"], 1);
        assert_eq!(hits["jpx"], 1);
    }

    #[test]
    fn the_report_names_the_reason_a_file_failed() {
        let text = run().to_report_json().to_pretty();
        assert!(text.contains("\"reason\": \"boom\""), "{text}");
        assert!(text.contains("\"outcome\": \"timed_out\""), "{text}");
    }

    /// The settings are in both documents. A pass rate measured at 72 dpi
    /// without fonts and one measured at 300 dpi with them are different
    /// measurements, and a committed number that does not say which is one
    /// nobody can reproduce.
    #[test]
    fn both_documents_record_how_the_run_was_configured() {
        let run = run();
        for text in [
            run.to_report_json().to_pretty(),
            run.to_ratchet_json("n").to_pretty(),
        ] {
            assert!(text.contains("\"fonts\": \"none\""), "{text}");
            assert!(text.contains("\"timeout_seconds\": 20"), "{text}");
            assert!(text.contains("\"dpi\": 72"), "{text}");
        }
    }

    /// No silent caps. What a run skipped travels in the documents, not in
    /// the console output that scrolled past.
    #[test]
    fn a_truncated_run_says_so_in_both_documents() {
        let mut run = run();
        run.limits
            .push("verapdf: sampled the first 100 files".to_string());
        assert!(!run.complete());
        for text in [
            run.to_report_json().to_pretty(),
            run.to_ratchet_json("n").to_pretty(),
        ] {
            assert!(text.contains("\"complete\": false"), "{text}");
            assert!(text.contains("sampled the first 100"), "{text}");
        }
        let summary = run.summary_lines().join("\n");
        assert!(summary.contains("INCOMPLETE"), "{summary}");
    }

    #[test]
    fn the_capability_table_has_a_row_per_corpus_and_a_total() {
        let table = run().capability_table();
        assert!(table.contains("| `synthetic` | 5 |"), "{table}");
        assert!(table.contains("jbig2"), "{table}");
        assert!(table.contains("**all** | **5**"), "{table}");
    }
}
