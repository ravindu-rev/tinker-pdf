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

use crate::json::Json;
use crate::report::Run;

/// One corpus's recorded bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bar {
    pub name: String,
    pub total: u64,
    pub passed: u64,
    pub degraded: u64,
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
        if passed > total || degraded > total {
            return Err(format!(
                "`{name}` records {passed} passed and {degraded} degraded out \
                 of {total}, which cannot be"
            ));
        }
        if bars.iter().any(|b: &Bar| b.name == name) {
            return Err(format!("`{name}` appears twice in the ratchet"));
        }
        bars.push(Bar {
            name,
            total,
            passed,
            degraded,
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
    use crate::runner::{FileResult, Outcome};
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
                pages: 1,
                rendered: 1,
                warnings,
                capabilities: BTreeSet::new(),
                millis: 1,
            });
        }
        out
    }

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
            }],
            complete: true,
            fonts: "none".to_string(),
        }
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
            (good.replace("\"schema\": 1", "\"schema\": 7"), "schema 7"),
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
