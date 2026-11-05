//! The determinism roster (K-07): every test that holds a fingerprint across
//! targets, and the CI legs that must say each one passed.
//!
//! Ruling 4 is proved by tests that hash what the engine made and compare it
//! with a value committed in the tree, on every target CI runs. Such a test
//! proves nothing on a target whose log does not show it passing: an
//! `#[ignore]` added one afternoon, a `cfg` that leaves a target out, or a
//! filter matching nothing all leave a green tick behind them. So each test is
//! named here, and `cargo xtask determinism` holds three things: it is a
//! `#[test]` in its file with no `#[ignore]`; the native `test` legs and the
//! `wasm-determinism` job each grep it by name; and every name those jobs grep
//! as passing is one of these. A row that lands a fingerprint adds its entry
//! and its greps in one commit.

use std::collections::BTreeSet;
use std::path::Path;

/// One test the CI legs must show passing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    /// Its file, from the repository root.
    pub file: &'static str,
    /// Its name, as the legs grep it.
    pub test: &'static str,
}

/// Every cross-target fingerprint test, and the tests the legs grep beside
/// them.
pub const ROSTER: &[Fingerprint] = &[
    // The render and document hashes, and the display list's replay of every
    // fingerprinted page.
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "rendering_is_stable_across_targets",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "the_synthesised_document_is_the_same_bytes_on_every_target",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "the_synthesised_fixed_document_is_the_same_bytes_on_every_target",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "the_synthesised_book_is_the_same_bytes_on_every_target",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/determinism.rs",
        test: "a_display_list_replays_every_fingerprinted_page_byte_for_byte",
    },
    // The archival packet's hash.
    Fingerprint {
        file: "crates/tinker-pdf/tests/pdfa_writer.rs",
        test: "the_archival_fixture_hashes_the_same_on_every_target",
    },
    // The shaped page's pixels.
    Fingerprint {
        file: "crates/tinker-pdf/tests/epub_shaped.rs",
        test: "the_shaped_page_renders_to_the_bytes_it_always_did",
    },
    // The render options' pinned hashes.
    Fingerprint {
        file: "crates/tinker-pdf/tests/render_options.rs",
        test: "ink_output_is_pinned",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/render_options.rs",
        test: "transparent_and_premultiplied_output_is_pinned",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/render_options.rs",
        test: "hard_edged_output_is_pinned",
    },
    // MS-01's guard suite: its roster of guards, and the work counts it pins
    // for every target.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "every_guard_is_landed_or_owed",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "the_tally_counts_the_same_on_every_target",
    },
    // MS-01's A1 lookup guard: the nodes one page lookup reads, held on every
    // target to its root and depth x fan-out under it.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a1_page_lookup_reads_at_most_depth_times_fan_out_nodes",
    },
    // MS-01's A1 total, A2 and A10 guards: a page loop over the 10 000-page
    // tree within 3 x 10 000 visits, every A2 leg's work linear in its pages,
    // and the allocations a page of three page loops costs flat in the pages.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a1_every_page_of_ten_thousand_costs_at_most_thirty_thousand_visits",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_text_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_render_all_at_36_dpi_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_rewrite_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_subset_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_validation_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guard_allocations.rs",
        test: "a10_page_lookup_allocates_a_flat_amount_per_page",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guard_allocations.rs",
        test: "a10_text_allocates_a_flat_amount_per_page",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guard_allocations.rs",
        test: "a10_render_all_at_36_dpi_allocates_a_flat_amount_per_page",
    },
    // MS-01's A2 editor legs: every page turned, a second document's every
    // page imported, two halves kept and saved, and every page redacted.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_rotate_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_merge_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_split_is_linear_in_pages",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a2_redaction_is_linear_in_pages",
    },
    // MS-01's A5 merge leg: no stream decoded twice in a merge's save.
    Fingerprint {
        file: "crates/tinker-pdf/tests/guards.rs",
        test: "a5_a_merge_decodes_each_stream_at_most_once",
    },
    // MS-08's counted guard: the requests page one of the linearized fixture
    // and of its unlinearized twin makes, measured once and held on every
    // target beside the row's ceilings.
    Fingerprint {
        file: "crates/tinker-pdf/tests/streaming_open.rs",
        test: "a_linearized_first_page_costs_at_most_six_requests_and_never_its_main_table",
    },
    Fingerprint {
        file: "crates/tinker-pdf/tests/streaming_open.rs",
        test: "the_same_pages_unlinearized_cost_their_tail_table_and_page_one_in_at_most_twelve_requests",
    },
    // The shaper's own fingerprints.
    Fingerprint {
        file: "crates/tinker-pdf-shape/tests/fingerprints.rs",
        test: "shaping_is_stable_across_targets",
    },
    Fingerprint {
        file: "crates/tinker-pdf-shape/tests/fingerprints.rs",
        test: "bidi_is_stable_across_targets",
    },
];

/// The workflow the legs are jobs of.
pub const CI_WORKFLOW: &str = ".github/workflows/ci.yml";

/// The jobs whose logs must show every roster test passing: the native
/// `test` matrix, whose steps every one of its three legs runs, and the
/// wasm32-wasip1 job.
pub const LEGS: [&str; 2] = ["test", "wasm-determinism"];

/// `cargo xtask determinism`: the roster against its files and the legs.
pub fn check(root: &Path) -> Result<(), Vec<String>> {
    let workflow = std::fs::read_to_string(root.join(CI_WORKFLOW))
        .map_err(|e| vec![format!("{CI_WORKFLOW}: {e}")])?;
    let mut problems = Vec::new();
    for entry in ROSTER {
        match std::fs::read_to_string(root.join(entry.file)) {
            Ok(source) => problems.extend(unignored(&source, entry)),
            Err(e) => problems.push(format!("{}: {e}", entry.file)),
        }
    }
    let names: Vec<&str> = ROSTER.iter().map(|entry| entry.test).collect();
    problems.extend(legs(&workflow, &names));
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Whether `entry` is a `#[test]` in `source` with no `#[ignore]` among the
/// attributes above it, as a problem naming what is wrong.
fn unignored(source: &str, entry: &Fingerprint) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    let signature = format!("fn {}(", entry.test);
    let Some(at) = lines
        .iter()
        .position(|line| line.trim_start().starts_with(&signature))
    else {
        return Some(format!(
            "{}: the roster names `{}` and the file has no such test",
            entry.file, entry.test
        ));
    };
    // The attributes and doc comment directly above the function.
    let above: Vec<&str> = lines[..at]
        .iter()
        .rev()
        .map(|line| line.trim())
        .take_while(|line| line.starts_with("#[") || line.starts_with("///"))
        .collect();
    if !above.contains(&"#[test]") {
        return Some(format!("{}: `{}` is not a #[test]", entry.file, entry.test));
    }
    if above.iter().any(|line| line.starts_with("#[ignore")) {
        return Some(format!(
            "{}: `{}` is #[ignore]d, so no leg runs it",
            entry.file, entry.test
        ));
    }
    None
}

/// The problems with the legs: a roster test a leg does not grep as passing,
/// and a test a leg greps as passing that the roster does not name.
pub fn legs(workflow: &str, roster: &[&str]) -> Vec<String> {
    let mut problems = Vec::new();
    for leg in LEGS {
        let Some(text) = job(workflow, leg) else {
            problems.push(format!("{CI_WORKFLOW}: there is no `{leg}` job"));
            continue;
        };
        let grepped = grepped(text);
        for name in roster {
            if !grepped.contains(*name) {
                problems.push(format!(
                    "{CI_WORKFLOW}: the `{leg}` job does not grep `{name}` as passing, so \
                     nothing says it ran there"
                ));
            }
        }
        for name in &grepped {
            if !roster.contains(&name.as_str()) {
                problems.push(format!(
                    "{CI_WORKFLOW}: the `{leg}` job greps `{name}`, which the determinism \
                     roster does not name"
                ));
            }
        }
    }
    problems
}

/// One job's text: from its `  <name>:` line under `jobs:` to the next job.
fn job<'a>(workflow: &'a str, name: &str) -> Option<&'a str> {
    let header = format!("  {name}:");
    let mut start = None;
    let mut offset = 0usize;
    for line in workflow.split_inclusive('\n') {
        let trimmed = line.trim_end();
        let is_job = trimmed.starts_with("  ")
            && !trimmed.starts_with("   ")
            && trimmed.ends_with(':')
            && !trimmed.trim_start().starts_with('#');
        match start {
            None if trimmed == header => start = Some(offset),
            Some(begin) if is_job => return workflow.get(begin..offset),
            _ => {}
        }
        offset += line.len();
    }
    start.and_then(|begin| workflow.get(begin..))
}

/// The tests a job's steps grep as passing: a quoted pattern ending in
/// ` [.]{3} ok` or ` \.\.\. ok`, its name the word before, after an optional
/// `test `.
fn grepped(job: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in job.lines() {
        if !line.contains("grep") {
            continue;
        }
        for pattern in line.split('\'').skip(1).step_by(2) {
            let Some(head) = pattern
                .strip_suffix(" [.]{3} ok")
                .or_else(|| pattern.strip_suffix(" \\.\\.\\. ok"))
            else {
                continue;
            };
            let name = head.strip_prefix("test ").unwrap_or(head);
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                out.insert(name.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A workflow of the two legs, each grepping `names`.
    fn workflow(native: &[&str], wasm: &[&str]) -> String {
        let greps = |names: &[&str], style: bool| -> String {
            names
                .iter()
                .map(|name| {
                    if style {
                        format!("          grep -Eq '{name} [.]{{3}} ok' determinism.log\n")
                    } else {
                        format!("          grep -q 'test {name} \\.\\.\\. ok' determinism.log\n")
                    }
                })
                .collect()
        };
        format!(
            "jobs:\n  test:\n    steps:\n      - run: |\n{}  wasm-determinism:\n    steps:\n      - run: |\n{}  wasm:\n    steps:\n          grep -q 'elsewhere \\.\\.\\. ok' x.log\n",
            greps(native, true),
            greps(wasm, false)
        )
    }

    /// A roster test one leg does not grep fails, naming the leg and the
    /// test; both legs grepping every one passes.
    #[test]
    fn a_fingerprint_test_a_leg_does_not_grep_fails() {
        let roster = ["rendering_is_stable_across_targets", "ink_output_is_pinned"];
        assert!(legs(&workflow(&roster, &roster), &roster).is_empty());

        let problems = legs(
            &workflow(&roster, &["rendering_is_stable_across_targets"]),
            &roster,
        );
        assert_eq!(problems.len(), 1, "{problems:#?}");
        assert!(
            problems[0].contains("`wasm-determinism` job does not grep `ink_output_is_pinned`"),
            "{problems:#?}"
        );
        // A grep in another job is not a leg's.
        assert!(!problems.iter().any(|p| p.contains("elsewhere")));
    }

    /// A test a leg greps as passing that the roster does not name fails, so
    /// a fingerprint cannot be added to the legs and left out of the roster.
    #[test]
    fn a_grep_naming_no_listed_test_fails() {
        let roster = ["rendering_is_stable_across_targets"];
        let both = ["rendering_is_stable_across_targets", "a_new_hash_is_pinned"];
        let problems = legs(&workflow(&both, &roster), &roster);
        assert_eq!(problems.len(), 1, "{problems:#?}");
        assert!(
            problems[0].contains("`test` job greps `a_new_hash_is_pinned`"),
            "{problems:#?}"
        );
    }

    /// A roster test must be a `#[test]` its file holds, and not `#[ignore]`d.
    #[test]
    fn a_roster_test_is_present_and_not_ignored() {
        let entry = Fingerprint {
            file: "x.rs",
            test: "a_hash_is_pinned",
        };
        let present = "/// Pinned.\n#[test]\nfn a_hash_is_pinned() {}\n";
        assert_eq!(unignored(present, &entry), None);
        let ignored = "#[test]\n#[ignore = \"slow\"]\nfn a_hash_is_pinned() {}\n";
        assert!(unignored(ignored, &entry).is_some_and(|p| p.contains("#[ignore]d")));
        let plain = "fn a_hash_is_pinned() {}\n";
        assert!(unignored(plain, &entry).is_some_and(|p| p.contains("is not a #[test]")));
        assert!(unignored("", &entry).is_some_and(|p| p.contains("no such test")));
    }

    /// The committed workflow and roster agree.
    #[test]
    fn the_committed_roster_holds() {
        check(&crate::repo_root()).unwrap_or_else(|problems| panic!("{problems:#?}"));
    }
}
