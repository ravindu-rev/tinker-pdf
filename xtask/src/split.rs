//! `cargo xtask search-split`: where prebuilt-text search spends its time.
//!
//! MS-04's two searches of the 100 pages' prebuilt text each have, beside
//! them in the text group, an operation for every phase of the work
//! `TextPage::search` and `TextPage::search_with` do on every call: folding
//! and joining each line, the whole-word search's UAX #29 boundaries,
//! `str::find` with its restarts, and mapping each hit back to glyphs
//! (`crates/tinker-pdf/benches/text.rs`). This reads a run's estimates,
//! criterion's own as `cargo xtask bench-check` reads them, and prints each
//! search's time a call beside each phase's time a call and share of it.
//!
//! **The sum check is what keeps the split honest.** It fails where a phase
//! or a search is missing from the run, and where the phases together do not
//! come within a tenth of their search: a phase the split leaves out, or time
//! it counts twice, moves the sum before it moves anything else.
//!
//! The figures are the operations' own: a run's estimates, and once the
//! named machine has measured the set, its entry in `baseline.json` (ruling
//! 16). Nothing is spawned and nothing is timed here.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use crate::bench::{by_listed_id, groups, measured, Group, BASELINE_PATH};

/// The group the searches and their phases are operations of.
pub const GROUP: &str = "text";

/// Each search the split is taken of, by its name in the text group's list,
/// and the names of its phases, in the order the search runs them.
pub const SEARCHES: [(&str, &[&str]); 2] = [
    (
        "search 100 pages' prebuilt text",
        &[
            "plain prebuilt search, folding and joining",
            "plain prebuilt search, find and its restarts",
            "plain prebuilt search, mapping hits to glyphs",
        ],
    ),
    (
        "search 100 pages' prebuilt text, whole words, unaccented",
        &[
            "unaccented whole-word search, folding and joining",
            "unaccented whole-word search, UAX 29 word boundaries",
            "unaccented whole-word search, find and its restarts",
            "unaccented whole-word search, mapping hits to glyphs",
        ],
    ),
];

/// How far the phases together may sit from their search, as a fraction of
/// it: a tenth.
pub const TOLERANCE: f64 = 0.1;

/// The task: the split of the run whose estimates are under
/// `target/criterion`, printed, or why it cannot be taken or does not sum.
///
/// # Errors
///
/// An option given, a `baseline.json` that does not read, or what [`split`]
/// refuses.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    if let Some(option) = args.first() {
        return Err(format!(
            "search-split takes no options, and was given `{option}`"
        ));
    }
    let text = std::fs::read_to_string(root.join(BASELINE_PATH))
        .map_err(|error| format!("{BASELINE_PATH}: {error}"))?;
    let lists = groups(&text, BASELINE_PATH).map_err(|error| error.to_string())?;
    let now = by_listed_id(measured(&root.join("target").join("criterion")), &lists);
    let report = split(&lists, &now)?;
    print!("{report}");
    Ok(())
}

/// The split of a run whose means, in nanoseconds an iteration, `now` holds
/// by listed id: each search's milliseconds a call, and each phase's with its
/// share of the search, a call being an iteration's time over the `calls`
/// its list gives it, so that a phase sized to a call count of its own
/// still divides by what it does.
///
/// # Errors
///
/// A search or a phase the lists do not hold, one the run has no estimate
/// for, or a search whose phases together are not within [`TOLERANCE`] of
/// it — this last with the whole report before it, so that what failed can
/// be read.
pub fn split(lists: &[Group], now: &BTreeMap<String, f64>) -> Result<String, String> {
    let group = lists
        .iter()
        .find(|list| list.name == GROUP)
        .ok_or_else(|| format!("{BASELINE_PATH} lists no `{GROUP}` group"))?;
    let per_call = |name: &str| -> Result<f64, String> {
        let listed = group
            .operations
            .iter()
            .find(|operation| operation.name == name)
            .ok_or_else(|| format!("the `{GROUP}` list in {BASELINE_PATH} holds no `{name}`"))?;
        let id = group.id(name);
        let nanos = now.get(&id).ok_or_else(|| {
            format!(
                "the run holds no estimate for `{id}`: run `cargo bench -p tinker-pdf --bench \
                 {GROUP}` first"
            )
        })?;
        Ok(nanos / listed.calls as f64 / 1_000_000.0)
    };

    let mut report =
        String::from("search-split: RAN, each phase's time a call and its share of its search\n");
    let mut unmet = Vec::new();
    for (search, phases) in SEARCHES {
        let whole = per_call(search)?;
        let _ = writeln!(report, "  {GROUP}/{search}: {whole:.3} ms a call");
        let mut together = 0.0;
        for phase in phases {
            let ms = per_call(phase)?;
            together += ms;
            let _ = writeln!(report, "    {phase}: {ms:.3} ms, {:.1} %", share(ms, whole));
        }
        let _ = writeln!(
            report,
            "    the phases together: {together:.3} ms, {:.1} %",
            share(together, whole)
        );
        let within = whole > 0.0 && (together / whole - 1.0).abs() <= TOLERANCE;
        if !within {
            unmet.push(format!(
                "the phases of `{GROUP}/{search}` come to {:.1} % of it, outside a tenth of it: \
                 a phase is missing from the split or counted twice",
                share(together, whole)
            ));
        }
    }
    if unmet.is_empty() {
        Ok(report)
    } else {
        Err(format!("{report}{}", unmet.join("\n")))
    }
}

/// `part` as a percentage of `whole`.
fn share(part: f64, whole: f64) -> f64 {
    100.0 * part / whole
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo_root;

    /// The committed lists.
    fn committed() -> Vec<Group> {
        let text = std::fs::read_to_string(repo_root().join(BASELINE_PATH)).expect("the file");
        groups(&text, BASELINE_PATH).expect("it parses")
    }

    /// A run's means for the two searches and their phases: each search a
    /// call the figure given, each phase the fraction given of its search's,
    /// as an iteration of its own calls.
    fn run_of(lists: &[Group], fractions: &[&[f64]; 2]) -> BTreeMap<String, f64> {
        let group = lists.iter().find(|list| list.name == GROUP).expect("text");
        let calls = |name: &str| {
            group
                .operations
                .iter()
                .find(|operation| operation.name == name)
                .expect("listed")
                .calls as f64
        };
        let mut now = BTreeMap::new();
        for ((search, phases), (fractions, ms)) in
            SEARCHES.iter().zip(fractions.iter().zip([36.0, 77.0]))
        {
            now.insert(group.id(search), ms * 1e6 * calls(*search));
            for (phase, fraction) in phases.iter().zip(fractions.iter()) {
                now.insert(group.id(phase), fraction * ms * 1e6 * calls(phase));
            }
        }
        now
    }

    /// Every name the split reads is an operation of the committed text
    /// list, each phase sized as its search is, so a renamed operation fails
    /// here rather than on the run that needed it.
    #[test]
    fn the_split_names_operations_the_text_group_lists() {
        let lists = committed();
        let group = lists.iter().find(|list| list.name == GROUP).expect("text");
        let listed = |name: &str| {
            group
                .operations
                .iter()
                .find(|operation| operation.name == name)
                .unwrap_or_else(|| panic!("the text list holds no `{name}`"))
        };
        for (search, phases) in SEARCHES {
            let whole = listed(search);
            for phase in phases {
                let part = listed(phase);
                assert_eq!(
                    (part.per_iteration, part.calls),
                    (whole.per_iteration, whole.calls),
                    "`{phase}` covers what `{search}` covers"
                );
            }
        }
    }

    /// Phases that sum to their search, or within a tenth of it either way,
    /// split: each share printed beside its phase, and the RAN banner.
    #[test]
    fn phases_within_a_tenth_of_their_search_split() {
        let lists = committed();
        for fractions in [
            [&[0.6, 0.3, 0.1][..], &[0.5, 0.2, 0.2, 0.1][..]],
            [&[0.6, 0.3, 0.19][..], &[0.5, 0.2, 0.15, 0.06][..]],
        ] {
            let report = split(&lists, &run_of(&lists, &fractions)).expect("it splits");
            assert!(report.starts_with("search-split: RAN"), "{report}");
            assert!(
                report.contains("plain prebuilt search, folding and joining: 21.600 ms, 60.0 %"),
                "{report}"
            );
            assert!(
                report.contains("text/search 100 pages' prebuilt text: 36.000 ms a call"),
                "{report}"
            );
        }
    }

    /// Phases a tenth short of their search, or a tenth over it, fail the
    /// split by the search they belong to, the report printed before it.
    #[test]
    fn phases_outside_a_tenth_of_their_search_fail() {
        let lists = committed();
        for (fractions, failing) in [
            (
                [&[0.5, 0.3, 0.09][..], &[0.5, 0.2, 0.2, 0.1][..]],
                "search 100 pages' prebuilt text`",
            ),
            (
                [&[0.6, 0.3, 0.1][..], &[0.6, 0.3, 0.2, 0.1][..]],
                "whole words, unaccented`",
            ),
        ] {
            let error = split(&lists, &run_of(&lists, &fractions)).expect_err("it fails");
            assert!(error.starts_with("search-split: RAN"), "{error}");
            assert!(error.contains("outside a tenth of it"), "{error}");
            assert!(error.contains(failing), "{error}");
        }
    }

    /// A run missing a search or one of its phases is refused by name, and
    /// so is a list that does not hold one.
    #[test]
    fn a_missing_phase_or_search_is_refused_by_name() {
        let lists = committed();
        let whole = run_of(&lists, &[&[0.6, 0.3, 0.1][..], &[0.5, 0.2, 0.2, 0.1][..]]);
        for (search, phases) in SEARCHES {
            for name in std::iter::once(search).chain(phases.iter().copied()) {
                let mut short = whole.clone();
                short.remove(&format!("{GROUP}/{name}"));
                let error = split(&lists, &short).expect_err("refused");
                assert!(
                    error.contains(&format!("no estimate for `{GROUP}/{name}`")),
                    "{error}"
                );
            }
        }
        let mut unlisted = committed();
        for list in &mut unlisted {
            list.operations
                .retain(|operation| operation.name != SEARCHES[1].1[1]);
        }
        let error = split(&unlisted, &whole).expect_err("refused");
        assert!(
            error.contains("holds no `unaccented whole-word search, UAX 29"),
            "{error}"
        );
    }

    /// Options are refused, since the split is of the one run there is.
    #[test]
    fn the_task_takes_no_options() {
        let error = run(&repo_root(), &["--machine".to_string()]).expect_err("refused");
        assert!(error.contains("takes no options"), "{error}");
    }
}
