//! The bar, and what it means to clear it.
//!
//! ## Why the arithmetic is integers
//!
//! The obvious comparator divides and compares: `passed_now / total_now >=
//! passed_before / total_before`. It is wrong in two ways at once.
//!
//! The small way is that binary floating point cannot represent most of these
//! ratios, so a run that passed exactly the same files can compare as very
//! slightly worse and fail; the usual patch is an epsilon, which is a hole of
//! a size nobody chose.
//!
//! The large way is what happens when a corpus grows. `960/976` is 0.9836.
//! Add forty upstream files, pass thirty of them, and the run is `990/1016` =
//! 0.9744 — a regression, correctly, because ten new files fail. But if the
//! comparison is written the other way round anywhere, or a rate is stored
//! instead of a pair of counts, growth *dilutes* the bar and a run that broke
//! files can clear it. Storing counts and cross-multiplying removes the
//! division entirely:
//!
//! ```text
//! passed_now * total_before >= passed_before * total_now
//! ```
//!
//! Both sides are exact integers, there is no epsilon, and a corpus whose
//! size changed is compared on the same terms as one that did not.
//!
//! ## What is ratcheted and what is only reported
//!
//! The **pass rate** is the bar: a regression fails CI, which is what plan 23
//! asks for.
//!
//! The **degradation rate** — the share of files that rendered with something
//! reported — is compared and printed, and does not fail by default. It is the
//! honest choice rather than the lenient one: a release that starts *naming* a
//! leniency it previously performed in silence makes this number go up, and
//! that is an improvement in exactly the property ruling 10 is about. Failing
//! on it would put a standing incentive on not reporting. `--strict` turns it
//! into a failure for anyone who wants that trade.
//!
//! ## What is refused rather than compared
//!
//! A comparison whose two sides are not the same measurement is refused, not
//! resolved. An incomplete run, a corpus that vanished, and a run whose
//! `--fonts` setting differs from the recorded one all stop the comparison
//! with a message. The font case is the subtle one: this engine bundles no
//! faces, so supplying them removes most of the warnings, and letting that
//! land in the same slot as a no-faces figure would either look like a large
//! improvement or bake in a bar no plain run can clear.

use std::collections::BTreeMap;

use crate::json::Json;
use crate::report::Run;

/// One corpus's recorded bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bar {
    pub name: String,
    pub total: u64,
    pub passed: u64,
    pub degraded: u64,
    /// How many files the strict pass ran on (ruling 13): opened cleanly,
    /// unencrypted, and rewritten.
    pub strict_eligible: u64,
    /// How many of those rewrites carried no structural defect.
    pub strict_clean: u64,
    /// Per metamorphic relation (roadmap step 7): how many files it was asked
    /// of, and how many it held on.
    ///
    /// A map rather than fields, because the relations are a list that grows:
    /// a bar recorded before a relation existed simply has no entry for it,
    /// and a run that adds one is an improvement rather than a refusal.
    pub metamorphic: BTreeMap<String, (u64, u64)>,
    /// What the structure tree walk reached (ISO 32000-1 14.7), or `None` in a
    /// bar recorded before the walk existed.
    ///
    /// `None` is an improvement rather than a refusal, for the reason an
    /// absent metamorphic relation is: a bar that predates a measurement
    /// cannot be regressed against.
    pub tagged: Option<TaggedBar>,
}

/// The structure-tree bar for one corpus.
///
/// Four counts, compared three ways, and none of them a stored rate — see
/// [`compare`] for which comparison each takes and why. The counts are over
/// the corpus, not averaged over its files: a per-file rate averaged is not
/// the rate over the corpus, and a corpus of mostly-untagged files would
/// otherwise let one enormous tree carry the figure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaggedBar {
    /// Files carrying a `/StructTreeRoot` this engine could read.
    pub files: u64,
    /// Structure elements reached by the `/K` walk, summed.
    pub elements: u64,
    /// Characters a structure element claimed.
    pub matched: u64,
    /// Characters carrying an `/MCID` no element on their page claimed.
    pub orphans: u64,
}

/// A committed ratchet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ratchet {
    pub bars: Vec<Bar>,
    /// Whether the run that produced it covered everything.
    pub complete: bool,
    /// The `--fonts` setting it was measured under.
    pub fonts: String,
}

impl Ratchet {
    fn bar(&self, name: &str) -> Option<&Bar> {
        self.bars.iter().find(|b| b.name == name)
    }
}

/// The comparison itself: does the bar still hold?
///
/// Integers throughout. See the module note for why this is not a division.
#[must_use]
pub fn holds(passed_before: u64, total_before: u64, passed_now: u64, total_now: u64) -> bool {
    // `u128` because nothing forces a corpus to stay small, and a silent wrap
    // in the comparator that decides whether the engine regressed is the last
    // place anyone would look.
    let left = u128::from(passed_now) * u128::from(total_before);
    let right = u128::from(passed_before) * u128::from(total_now);
    left >= right
}

/// What a comparison found.
#[derive(Clone, Debug, Default)]
pub struct Comparison {
    /// Reasons the run is worse than the bar. Any entry fails CI.
    pub regressions: Vec<String>,
    /// Reasons the comparison could not be made at all. Also fails, and
    /// separately, because "refused to compare" and "compared and lost" want
    /// different fixes.
    pub refusals: Vec<String>,
    /// Numbers that moved the right way, or corpora that are new.
    pub improvements: Vec<String>,
    /// Everything else worth printing, degradation included.
    pub notes: Vec<String>,
}

impl Comparison {
    pub fn failed(&self) -> bool {
        !self.regressions.is_empty() || !self.refusals.is_empty()
    }
}

/// Compares a run against the committed bar.
pub fn compare(before: &Ratchet, now: &Run, strict: bool) -> Comparison {
    let mut out = Comparison::default();

    // Refused before anything is compared. A sampled run's numbers are not
    // wrong, they are about a different set of files, and letting them lower
    // the bar is how a corpus quietly stops being a corpus.
    if !now.complete() {
        out.refusals.push(format!(
            "this run is incomplete, so its counts are not comparable with a \
             recorded bar: {}",
            now.limits.join("; ")
        ));
    }
    if !before.complete {
        out.refusals.push(
            "the committed ratchet was recorded from an incomplete run and \
             cannot be a bar; re-record it from a complete one"
                .to_string(),
        );
    }
    if before.fonts != now.settings.fonts {
        out.refusals.push(format!(
            "the bar was recorded with fonts `{}` and this run used `{}`; the \
             two are different measurements, because this engine bundles no \
             faces and supplying them removes most of the warnings",
            before.fonts, now.settings.fonts
        ));
    }

    for bar in &before.bars {
        let Some(corpus) = now.corpora.iter().find(|c| c.name == bar.name) else {
            out.refusals.push(format!(
                "`{}` is in the ratchet and was not run; a bar cannot be \
                 cleared by not attempting it",
                bar.name
            ));
            continue;
        };
        let (total, passed) = (corpus.total(), corpus.passed());

        // Checked before the cross-multiplication, which would otherwise
        // approve it: with `total_now` at zero both sides are zero and the
        // comparison holds, so a corpus that failed to extract would clear
        // every bar in the file.
        if total == 0 && bar.total > 0 {
            out.refusals.push(format!(
                "`{}` ran zero files against a bar of {}; that is an empty \
                 directory, not a pass rate",
                bar.name, bar.total
            ));
            continue;
        }

        if holds(bar.passed, bar.total, passed, total) {
            if passed * bar.total > bar.passed * total {
                out.improvements.push(format!(
                    "{}: {passed}/{total} passed, up from {}/{}",
                    bar.name, bar.passed, bar.total
                ));
            }
        } else {
            out.regressions.push(format!(
                "{}: {passed}/{total} passed, which is worse than the recorded \
                 {}/{} ({passed} * {} = {} < {} = {} * {total})",
                bar.name,
                bar.passed,
                bar.total,
                bar.total,
                u128::from(passed) * u128::from(bar.total),
                u128::from(bar.passed) * u128::from(total),
                bar.passed,
            ));
        }

        // The second axis. Lower is better, so the inequality is the mirror
        // of the one above: `degraded_now * total_before <= ...`.
        let degraded = corpus.degraded();
        let worse = u128::from(degraded) * u128::from(bar.total)
            > u128::from(bar.degraded) * u128::from(total);
        if worse {
            let message = format!(
                "{}: {degraded}/{total} rendered with warnings, up from {}/{}",
                bar.name, bar.degraded, bar.total
            );
            if strict {
                out.regressions.push(message);
            } else {
                out.notes
                    .push(format!("{message} (not a failure; --strict)"));
            }
        } else if degraded * bar.total < bar.degraded * total {
            out.improvements.push(format!(
                "{}: {degraded}/{total} rendered with warnings, down from {}/{}",
                bar.name, bar.degraded, bar.total
            ));
        }

        // The third axis (ruling 13): of the files this engine read cleanly,
        // how many produced a rewrite that holds up to ISO 32000 read
        // strictly. Only the structural tier counts, because a rewrite
        // inherits the page tree and the annotations from its source and a
        // backwards `/Rect` in somebody's 2003 invoice is the invoice's.
        //
        // Eligibility is compared as well as cleanliness. A change that made
        // fewer files readable would otherwise raise the rate by shrinking
        // the denominator, which is the same trick as sampling.
        let (eligible, clean) = (corpus.strict_eligible(), corpus.strict_clean());
        if eligible == 0 && bar.strict_eligible > 0 {
            out.regressions.push(format!(
                "{}: the strict pass ran on no file at all against a bar of {}",
                bar.name, bar.strict_eligible
            ));
        } else if !holds(bar.strict_clean, bar.strict_eligible, clean, eligible) {
            out.regressions.push(format!(
                "{}: {clean}/{eligible} rewrites validate, which is worse than the recorded {}/{}",
                bar.name, bar.strict_clean, bar.strict_eligible
            ));
        } else if u128::from(clean) * u128::from(bar.strict_eligible)
            > u128::from(bar.strict_clean) * u128::from(eligible)
        {
            out.improvements.push(format!(
                "{}: {clean}/{eligible} rewrites validate, up from {}/{}",
                bar.name, bar.strict_clean, bar.strict_eligible
            ));
        }
        if !holds(bar.strict_eligible, bar.total, eligible, total) {
            out.regressions.push(format!(
                "{}: the strict pass ran on {eligible}/{total} files, down from {}/{}; a smaller denominator is not a better rate",
                bar.name, bar.strict_eligible, bar.total
            ));
        }

        // The fourth axis (roadmap step 7): the metamorphic relations. Two
        // comparisons per relation and for the strict pass's reason — a
        // relation that quietly stopped being asked would otherwise raise its
        // own hold rate by shrinking what it is a rate of.
        let compared_now = corpus.metamorphic_compared();
        let held_now = corpus.metamorphic_held();
        for (relation, (compared_before, held_before)) in &bar.metamorphic {
            let compared = compared_now.get(relation).copied().unwrap_or(0);
            let held = held_now.get(relation).copied().unwrap_or(0);
            if compared == 0 && *compared_before > 0 {
                out.regressions.push(format!(
                    "{}: `{relation}` was asked of no file at all against a bar of {}",
                    bar.name, compared_before
                ));
                continue;
            }
            if !holds(*held_before, *compared_before, held, compared) {
                out.regressions.push(format!(
                    "{}: `{relation}` held on {held}/{compared}, worse than the \
                     recorded {held_before}/{compared_before}",
                    bar.name
                ));
            } else if u128::from(held) * u128::from(*compared_before)
                > u128::from(*held_before) * u128::from(compared)
            {
                out.improvements.push(format!(
                    "{}: `{relation}` held on {held}/{compared}, up from \
                     {held_before}/{compared_before}",
                    bar.name
                ));
            }
            if !holds(*compared_before, bar.total, compared, total) {
                out.regressions.push(format!(
                    "{}: `{relation}` was asked of {compared}/{total} files, down \
                     from {compared_before}/{}; declining the hard files is not a \
                     better rate",
                    bar.name, bar.total
                ));
            }
        }
        for relation in compared_now.keys() {
            if !bar.metamorphic.contains_key(relation) {
                out.improvements.push(format!(
                    "{}: `{relation}` held on {}/{} — new, with no recorded bar",
                    bar.name,
                    held_now.get(relation).copied().unwrap_or(0),
                    compared_now.get(relation).copied().unwrap_or(0),
                ));
            }
        }

        // The fifth axis (tagged PDF milestone 4): what the structure tree
        // walk reached. Three comparisons, because the three counts fail in
        // three different directions and one of them is not a floor.
        let now_tagged = corpus.tagged();
        match bar.tagged {
            // A first measurement of zero is not an improvement. It is a
            // corpus with no tagged files in it, or a walk that found none,
            // and the two are told apart by looking rather than by a word
            // that says the number went the right way.
            None if now_tagged.files == 0 => out.notes.push(format!(
                "{}: no file yielded a structure tree, and there is no recorded bar",
                bar.name
            )),
            None => out.improvements.push(format!(
                "{}: {} files yield a structure tree with {} elements — new, with no recorded bar",
                bar.name, now_tagged.files, now_tagged.elements
            )),
            Some(before) => {
                // (1) Files with a tree, as a share of the corpus. This is the
                // one the seeded regression trips: a walk that stops resolving
                // `/StructTreeRoot` finds no trees anywhere.
                if now_tagged.files == 0 && before.files > 0 {
                    out.regressions.push(format!(
                        "{}: no file yielded a structure tree at all, against a bar of {}",
                        bar.name, before.files
                    ));
                } else if !holds(before.files, bar.total, now_tagged.files, total) {
                    out.regressions.push(format!(
                        "{}: {}/{total} files yield a structure tree, down from {}/{}",
                        bar.name, now_tagged.files, before.files, bar.total
                    ));
                } else if u128::from(now_tagged.files) * u128::from(bar.total)
                    > u128::from(before.files) * u128::from(total)
                {
                    out.improvements.push(format!(
                        "{}: {}/{total} files yield a structure tree, up from {}/{}",
                        bar.name, now_tagged.files, before.files, bar.total
                    ));
                }

                // (2) Elements per file in the corpus, not per tagged file.
                // Per tagged file would let a walk that lost the large trees
                // hold its rate by also losing the small ones.
                if !holds(before.elements, bar.total, now_tagged.elements, total) {
                    out.regressions.push(format!(
                        "{}: the walk reached {} structure elements over {total} files, down from {} over {}",
                        bar.name, now_tagged.elements, before.elements, bar.total
                    ));
                }

                // (3) Orphans are the count that is better small, so the
                // comparison is on the share of marked characters the tree
                // *claimed* — matched against matched-plus-orphaned. An
                // absolute ceiling on orphans would be regressed by reading
                // more files, which is the opposite of what it is for.
                let (m, o) = (now_tagged.matched, now_tagged.orphans);
                if !holds(before.matched, before.matched + before.orphans, m, m + o) {
                    out.regressions.push(format!(
                        "{}: the tree claimed {m} of {} marked characters, a smaller share than the recorded {} of {}",
                        bar.name,
                        m + o,
                        before.matched,
                        before.matched + before.orphans
                    ));
                }
            }
        }
    }

    for corpus in &now.corpora {
        if before.bar(&corpus.name).is_none() {
            out.improvements.push(format!(
                "{}: {}/{} passed — new, with no recorded bar",
                corpus.name,
                corpus.passed(),
                corpus.total()
            ));
        }
    }

    out
}

/// Reads a committed ratchet.
pub fn parse(text: &str) -> Result<Ratchet, String> {
    let json = crate::json::parse(text)?;
    let schema = json
        .get("schema")
        .and_then(Json::as_u64)
        .ok_or("the ratchet has no `schema`")?;
    if schema != crate::report::SCHEMA {
        return Err(format!(
            "the ratchet is schema {schema} and this build writes schema {}; \
             a bar read under the wrong schema is not a bar",
            crate::report::SCHEMA
        ));
    }

    // Absent means false. A ratchet that forgot to say whether its run was
    // complete must not be assumed to have been.
    let complete = matches!(json.get("complete"), Some(Json::Bool(true)));
    let fonts = json
        .get("settings")
        .and_then(|s| s.get("fonts"))
        .and_then(Json::as_str)
        .ok_or("the ratchet does not say what `--fonts` it was measured with")?
        .to_string();

    let entries = json
        .get("corpora")
        .and_then(Json::as_array)
        .ok_or("the ratchet has no `corpora` array")?;
    let mut bars = Vec::new();
    for entry in entries {
        let name = entry
            .get("name")
            .and_then(Json::as_str)
            .ok_or("a corpus in the ratchet has no name")?
            .to_string();
        let count = |key: &str| -> Result<u64, String> {
            entry
                .get(key)
                .and_then(Json::as_u64)
                .ok_or_else(|| format!("`{name}` has no whole-number `{key}`"))
        };
        let total = count("total")?;
        let passed = count("passed")?;
        let degraded = count("degraded")?;
        let strict_eligible = count("strict_eligible")?;
        let strict_clean = count("strict_clean")?;
        let mut metamorphic = BTreeMap::new();
        if let Some(Json::Object(relations)) = entry.get("metamorphic") {
            for (relation, verdict) in relations {
                let read = |key: &str| -> Result<u64, String> {
                    verdict.get(key).and_then(Json::as_u64).ok_or_else(|| {
                        format!("`{name}`'s `{relation}` has no whole-number `{key}`")
                    })
                };
                let (compared, held) = (read("compared")?, read("held")?);
                if held > compared || compared > total {
                    return Err(format!(
                        "`{name}` records `{relation}` holding on {held} of {compared} \
                         files out of {total}, which cannot be"
                    ));
                }
                metamorphic.insert(relation.clone(), (compared, held));
            }
        }
        if passed > total || degraded > total {
            return Err(format!(
                "`{name}` records {passed} passed and {degraded} degraded out \
                 of {total}, which cannot be"
            ));
        }
        if strict_eligible > total || strict_clean > strict_eligible {
            return Err(format!(
                "`{name}` records {strict_clean} clean rewrites out of \
                 {strict_eligible} eligible of {total}, which cannot be"
            ));
        }
        let tagged = match entry.get("tagged") {
            None => None,
            Some(object) => {
                let read = |key: &str| -> Result<u64, String> {
                    object
                        .get(key)
                        .and_then(Json::as_u64)
                        .ok_or_else(|| format!("`{name}`'s `tagged` has no whole-number `{key}`"))
                };
                let tagged = TaggedBar {
                    files: read("files")?,
                    elements: read("elements")?,
                    matched: read("matched")?,
                    orphans: read("orphans")?,
                };
                if tagged.files > total {
                    return Err(format!(
                        "`{name}` records {} files with a structure tree out of {total}, which cannot be",
                        tagged.files
                    ));
                }
                // A bar with trees but no elements is what the seeded
                // regression writes, and it must be refused at the *bar*
                // rather than only compared against: a committed floor of
                // zero elements is a floor nothing can fall below.
                if tagged.files > 0 && tagged.elements == 0 {
                    return Err(format!(
                        "`{name}` records {} files with a structure tree and no structure elements at all, which is a floor nothing can fall below",
                        tagged.files
                    ));
                }
                Some(tagged)
            }
        };
        if bars.iter().any(|b: &Bar| b.name == name) {
            return Err(format!("`{name}` appears twice in the ratchet"));
        }
        bars.push(Bar {
            name,
            total,
            passed,
            degraded,
            strict_eligible,
            strict_clean,
            metamorphic,
            tagged,
        });
    }
    if bars.is_empty() {
        return Err("the ratchet records no corpora at all".to_string());
    }
    Ok(Ratchet {
        bars,
        complete,
        fonts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{CorpusReport, Settings};
    use crate::runner::{FileResult, Outcome, Tagged};
    use std::collections::{BTreeMap, BTreeSet};

    fn files(passed: u64, failed: u64, degraded: u64) -> Vec<FileResult> {
        let mut out = Vec::new();
        for index in 0..passed + failed {
            let outcome = if index < passed {
                Outcome::Passed
            } else {
                Outcome::Failed("no".to_string())
            };
            let warnings = if index < degraded {
                BTreeMap::from([("render:UnreadableFont".to_string(), 1usize)])
            } else {
                BTreeMap::new()
            };
            out.push(FileResult {
                path: format!("{index}.pdf"),
                outcome,
                cost: crate::runner::Cost::default(),
                bundled_faces: false,
                pages: 1,
                rendered: 1,
                warnings,
                capabilities: BTreeSet::new(),
                millis: 1,
                strict: crate::runner::Strict::Checked {
                    structure: 0,
                    semantics: 0,
                    kinds: BTreeMap::new(),
                },
                metamorphic: BTreeMap::new(),
                tagged: None,
            });
        }
        out
    }

    /// A run whose strict pass ran on every file and found nothing.
    fn run(name: &str, passed: u64, failed: u64, degraded: u64) -> Run {
        Run {
            corpora: vec![CorpusReport {
                name: name.to_string(),
                files: files(passed, failed, degraded),
            }],
            limits: Vec::new(),
            settings: Settings {
                timeout_seconds: 20,
                dpi: 72.0,
                fonts: "none".to_string(),
            },
        }
    }

    fn bar(name: &str, passed: u64, total: u64, degraded: u64) -> Ratchet {
        Ratchet {
            bars: vec![Bar {
                name: name.to_string(),
                total,
                passed,
                degraded,
                strict_eligible: total,
                strict_clean: total,
                metamorphic: BTreeMap::new(),
                tagged: None,
            }],
            complete: true,
            fonts: "none".to_string(),
        }
    }

    /// A run whose files carry the given structure counts, spread evenly.
    ///
    /// Evenly because the comparison is over corpus totals: how the elements
    /// are distributed across files cannot change any of the three answers,
    /// and a helper that pretended otherwise would be testing itself.
    fn tagged_run(name: &str, files_with_tree: u64, per_file: Tagged) -> Run {
        let mut run = run(name, 10, 0, 0);
        for (index, file) in run.corpora[0].files.iter_mut().enumerate() {
            if (index as u64) < files_with_tree {
                file.tagged = Some(per_file);
            }
        }
        run
    }

    fn tagged_bar(name: &str, tagged: TaggedBar) -> Ratchet {
        let mut committed = bar(name, 10, 10, 0);
        committed.bars[0].tagged = Some(tagged);
        committed
    }

    /// The seeded regression tagged PDF milestone 4 names: cap the walk's
    /// elements at zero and the check must fail. It fails twice over — the
    /// files-with-a-tree share collapses and the element floor is breached —
    /// and both messages are asserted, because a single message could be
    /// produced by a comparison that happened to be looking elsewhere.
    #[test]
    fn a_structure_walk_that_finds_nothing_is_a_regression() {
        let committed = tagged_bar(
            "verapdf",
            TaggedBar {
                files: 6,
                elements: 600,
                matched: 900,
                orphans: 100,
            },
        );
        let now = tagged_run("verapdf", 0, Tagged::default());
        let out = compare(&committed, &now, true);
        assert!(out.failed(), "{out:?}");
        assert!(
            out.regressions
                .iter()
                .any(|r| r.contains("no file yielded a structure tree at all")),
            "{:?}",
            out.regressions
        );
        assert!(
            out.regressions
                .iter()
                .any(|r| r.contains("structure elements")),
            "{:?}",
            out.regressions
        );
    }

    /// The subtler shape: every tree is still found and every element is still
    /// reached, but the join stopped claiming characters — a `/ParentTree` or
    /// `/MCID` regression that the element floor cannot see. The share of
    /// marked characters the tree claimed is what catches it.
    #[test]
    fn a_join_that_orphans_what_it_used_to_claim_is_a_regression() {
        let committed = tagged_bar(
            "verapdf",
            TaggedBar {
                files: 6,
                elements: 600,
                matched: 900,
                orphans: 100,
            },
        );
        let now = tagged_run(
            "verapdf",
            6,
            Tagged {
                elements: 100,
                matched: 50,
                orphans: 116,
                ..Tagged::default()
            },
        );
        let out = compare(&committed, &now, true);
        assert!(
            out.regressions
                .iter()
                .any(|r| r.contains("a smaller share")),
            "{:?}",
            out.regressions
        );
    }

    /// A run that reaches more of everything is not a regression, and the two
    /// improvements are printed rather than passed over in silence.
    #[test]
    fn reaching_more_structure_is_an_improvement() {
        let committed = tagged_bar(
            "verapdf",
            TaggedBar {
                files: 5,
                elements: 500,
                matched: 900,
                orphans: 100,
            },
        );
        let now = tagged_run(
            "verapdf",
            6,
            Tagged {
                elements: 100,
                matched: 190,
                orphans: 10,
                ..Tagged::default()
            },
        );
        let out = compare(&committed, &now, true);
        assert!(!out.failed(), "{out:?}");
        assert!(
            out.improvements
                .iter()
                .any(|i| i.contains("yield a structure tree, up from")),
            "{:?}",
            out.improvements
        );
    }

    /// A bar recorded before the walk existed cannot be regressed against, so
    /// the first run that measures one is an improvement and never a refusal.
    #[test]
    fn a_bar_with_no_structure_row_is_new_rather_than_broken() {
        let committed = bar("verapdf", 10, 10, 0);
        let now = tagged_run(
            "verapdf",
            6,
            Tagged {
                elements: 100,
                matched: 190,
                orphans: 10,
                ..Tagged::default()
            },
        );
        let out = compare(&committed, &now, true);
        assert!(!out.failed(), "{out:?}");
        assert!(
            out.improvements
                .iter()
                .any(|i| i.contains("new, with no recorded bar")),
            "{:?}",
            out.improvements
        );
    }

    /// The floor a seeded regression would otherwise be recorded *as*. A
    /// committed bar of zero elements over files that have trees is a floor
    /// nothing can fall below, so it is refused when the ratchet is read
    /// rather than compared against and passed.
    #[test]
    fn a_committed_bar_of_zero_elements_is_refused_at_the_ratchet() {
        let text = r#"{"schema":2,"complete":true,"settings":{"fonts":"none"},
            "corpora":[{"name":"verapdf","total":10,"passed":10,"degraded":0,
            "strict_eligible":10,"strict_clean":10,
            "tagged":{"files":6,"elements":0,"matched":0,"orphans":0}}]}"#;
        let error = parse(text).expect_err("a floor of zero is not a floor");
        assert!(error.contains("no structure elements at all"), "{error}");
    }

    /// Ruling 13's axis: a rewrite that stops validating is a regression, and
    /// so is a run that measured fewer files. The second is the one worth
    /// stating — a change that made fewer documents readable would otherwise
    /// raise the rate by shrinking its denominator.
    #[test]
    fn the_strict_axis_fails_both_ways() {
        let mut worse = run("pdfjs", 10, 0, 0);
        worse.corpora[0].files[0].strict = crate::runner::Strict::Checked {
            structure: 1,
            semantics: 0,
            kinds: BTreeMap::from([("free-head-missing".to_string(), 1usize)]),
        };
        let outcome = compare(&bar("pdfjs", 10, 10, 0), &worse, false);
        assert!(outcome.failed(), "{outcome:#?}");
        assert!(
            outcome.regressions[0].contains("9/10 rewrites validate"),
            "{:?}",
            outcome.regressions
        );

        let mut fewer = run("pdfjs", 10, 0, 0);
        fewer.corpora[0].files[0].strict =
            crate::runner::Strict::Ineligible("the file is encrypted".to_string());
        let outcome = compare(&bar("pdfjs", 10, 10, 0), &fewer, false);
        assert!(outcome.failed(), "{outcome:#?}");
        assert!(
            outcome.regressions[0].contains("ran on 9/10 files"),
            "{:?}",
            outcome.regressions
        );
    }

    /// And a semantic defect is not one: a rewrite inherits the page tree and
    /// the annotations from its source, so a backwards `/Rect` in somebody
    /// else's document is not this writer's regression.
    #[test]
    fn what_a_rewrite_inherited_does_not_move_the_bar() {
        let mut inherited = run("pdfjs", 10, 0, 0);
        inherited.corpora[0].files[0].strict = crate::runner::Strict::Checked {
            structure: 0,
            semantics: 4,
            kinds: BTreeMap::from([("annot-rect-unordered".to_string(), 4usize)]),
        };
        let outcome = compare(&bar("pdfjs", 10, 10, 0), &inherited, true);
        assert!(!outcome.failed(), "{outcome:#?}");
    }

    #[test]
    fn a_regression_fails() {
        let outcome = compare(&bar("pdfjs", 960, 976, 0), &run("pdfjs", 950, 26, 0), false);
        assert!(outcome.failed(), "{outcome:#?}");
        assert!(
            outcome.regressions[0].contains("950/976"),
            "{:?}",
            outcome.regressions
        );
    }

    #[test]
    fn an_improvement_passes_and_prints_the_numbers_to_paste() {
        let outcome = compare(&bar("pdfjs", 960, 976, 0), &run("pdfjs", 970, 6, 0), false);
        assert!(!outcome.failed(), "{outcome:#?}");
        assert!(
            outcome.improvements[0].contains("970/976 passed, up from 960/976"),
            "{:?}",
            outcome.improvements
        );
    }

    /// The heart of milestone 3. A corpus that grew is compared on the same
    /// terms as one that did not: the bar is a *ratio of counts*, and forty
    /// extra files do not lower it.
    ///
    /// 960/976 = 0.98361. The run below is 985/1016 = 0.96949 — worse, and it
    /// must fail even though more files passed in absolute terms. A
    /// comparator that compared `passed` alone, or that stored a rate and
    /// diluted it, would call this an improvement of twenty-five files.
    #[test]
    fn a_corpus_that_grew_does_not_reset_the_bar() {
        let outcome = compare(&bar("pdfjs", 960, 976, 0), &run("pdfjs", 985, 31, 0), false);
        assert!(
            outcome.failed(),
            "985 of 1016 is a lower rate than 960 of 976: {outcome:#?}"
        );

        // And the same growth carrying its weight passes: 1000/1016 = 0.98425.
        let outcome = compare(
            &bar("pdfjs", 960, 976, 0),
            &run("pdfjs", 1000, 16, 0),
            false,
        );
        assert!(!outcome.failed(), "{outcome:#?}");
    }

    /// The arithmetic itself, on the pairs that a float comparison gets
    /// wrong. 49999999/50000000 and 4999999/5000000 differ in the eighth
    /// decimal place; `f64` has room, `f32` does not, and nobody should have
    /// to know which one the comparator used.
    #[test]
    fn the_comparison_is_exact_rather_than_approximate() {
        assert!(holds(1, 3, 1, 3), "an unchanged rate holds");
        assert!(!holds(2, 3, 1, 2), "1/2 is below 2/3");
        assert!(holds(2, 3, 3, 4), "3/4 is above 2/3");
        assert!(
            !holds(4_999_999, 5_000_000, 49_999_989, 50_000_000),
            "a rate lower by one part in five million is still lower"
        );
        assert!(holds(4_999_999, 5_000_000, 49_999_990, 50_000_000));
    }

    /// A corpus that failed to extract runs zero files. Both sides of the
    /// cross-multiplication are then zero and the bar "holds" — so this is
    /// caught before the arithmetic, not by it.
    #[test]
    fn a_corpus_that_ran_nothing_is_refused_rather_than_passed() {
        let mut empty = run("pdfjs", 0, 0, 0);
        empty.corpora[0].files.clear();
        let outcome = compare(&bar("pdfjs", 960, 976, 0), &empty, false);
        assert!(outcome.failed(), "{outcome:#?}");
        assert!(
            outcome.refusals[0].contains("ran zero files"),
            "{:?}",
            outcome.refusals
        );
    }

    #[test]
    fn a_corpus_in_the_bar_and_not_in_the_run_is_refused() {
        let outcome = compare(&bar("verapdf", 100, 100, 0), &run("pdfjs", 10, 0, 0), false);
        assert!(outcome.failed(), "{outcome:#?}");
        assert!(
            outcome.refusals.iter().any(|r| r.contains("was not run")),
            "{:?}",
            outcome.refusals
        );
    }

    /// No silent caps, enforced rather than documented: a sampled run's
    /// numbers describe a different set of files, so they may not touch the
    /// bar in either direction.
    #[test]
    fn an_incomplete_run_may_not_be_compared_at_all() {
        let mut sampled = run("pdfjs", 970, 6, 0);
        sampled
            .limits
            .push("sampled the first 976 of 4000".to_string());
        let outcome = compare(&bar("pdfjs", 960, 976, 0), &sampled, false);
        assert!(outcome.failed(), "an improvement, and still refused");
        assert!(
            outcome.refusals[0].contains("incomplete"),
            "{:?}",
            outcome.refusals
        );
    }

    /// The correction that makes the second axis mean anything. A run with
    /// real faces has far fewer warnings than one without, and comparing the
    /// two would either read as a large improvement or set a bar no ordinary
    /// run can clear.
    #[test]
    fn a_run_with_fonts_is_not_compared_against_a_bar_without_them() {
        let mut with_faces = run("pdfjs", 960, 16, 10);
        with_faces.settings.fonts = "corpus/files/pdfjs/external/standard_fonts".to_string();
        let outcome = compare(&bar("pdfjs", 960, 976, 800), &with_faces, false);
        assert!(outcome.failed(), "{outcome:#?}");
        assert!(
            outcome.refusals[0].contains("different measurements"),
            "{:?}",
            outcome.refusals
        );
    }

    /// Degradation is reported both ways and fails only under `--strict`.
    #[test]
    fn degradation_is_a_note_by_default_and_a_regression_under_strict() {
        let worse = run("pdfjs", 976, 0, 900);
        let outcome = compare(&bar("pdfjs", 976, 976, 800), &worse, false);
        assert!(!outcome.failed(), "{outcome:#?}");
        assert!(
            outcome
                .notes
                .iter()
                .any(|n| n.contains("rendered with warnings, up from")),
            "{:?}",
            outcome.notes
        );

        let outcome = compare(&bar("pdfjs", 976, 976, 800), &worse, true);
        assert!(outcome.failed(), "{outcome:#?}");
    }

    #[test]
    fn a_ratchet_round_trips_through_its_own_writer() {
        let run = run("pdfjs", 960, 16, 100);
        let text = run.to_ratchet_json("a note").to_pretty();
        let read = parse(&text).expect("it parses");
        assert_eq!(read.bars[0].total, 976);
        assert_eq!(read.bars[0].passed, 960);
        assert_eq!(read.bars[0].degraded, 100);
        assert!(read.complete);
        assert_eq!(read.fonts, "none");
        assert!(
            !compare(&read, &run, true).failed(),
            "it clears its own bar"
        );
    }

    #[test]
    fn a_malformed_ratchet_is_rejected_with_a_useful_message() {
        let good = run("pdfjs", 960, 16, 100).to_ratchet_json("n").to_pretty();
        for (text, expected) in [
            (
                // Written against the constant rather than against a literal,
                // because a schema bump would otherwise turn this case into a
                // no-op and the test would pass by not testing anything.
                good.replace(
                    &format!("\"schema\": {}", crate::report::SCHEMA),
                    "\"schema\": 7",
                ),
                "schema 7",
            ),
            (
                good.replace("\"passed\": 960", "\"passed\": 9999"),
                "cannot be",
            ),
            (
                good.replace("\"fonts\": \"none\"", "\"fnots\": \"none\""),
                "--fonts",
            ),
            (good.replace("\"corpora\"", "\"korpora\""), "no `corpora`"),
            (
                good.replace("\"total\": 976", "\"total\": 97.6"),
                "whole-number",
            ),
        ] {
            let error = parse(&text).expect_err(&format!("must be rejected: {expected}"));
            assert!(
                error.contains(expected),
                "expected `{expected}`, got: {error}"
            );
        }
    }

    /// A ratchet that does not say whether its run was complete is not
    /// assumed to have been.
    #[test]
    fn a_ratchet_missing_its_completeness_flag_is_not_a_bar() {
        let text = run("pdfjs", 960, 16, 0)
            .to_ratchet_json("n")
            .to_pretty()
            .replace("\"complete\": true", "\"complete\": null");
        let read = parse(&text).expect("it still parses");
        assert!(!read.complete);
        assert!(compare(&read, &run("pdfjs", 970, 6, 0), false).failed());
    }
}
