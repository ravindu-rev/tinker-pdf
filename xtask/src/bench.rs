//! The speed ratchet: a committed baseline, and a comparison that can fail.
//!
//! # Why this exists rather than `cargo bench -- --baseline`
//!
//! Criterion compares against a saved baseline perfectly well and **never exits
//! non-zero when it loses**. The only failure path in 0.5.1 is a *missing*
//! baseline directory; a regression prints "Performance has regressed" and the
//! process exits 0. `bench.yml` had been scheduled weekly since it was written
//! on the strength of that comparison, so the job was reporting rather than
//! gating, and the roadmap's own row said so.
//!
//! So the comparison this repository ratchets on is one it owns. It reads
//! criterion's own `estimates.json` — the numbers criterion computed, not a
//! second timing of anything — and holds each operation to a committed figure
//! and a band.
//!
//! # Why a band, and why the band is measured
//!
//! A benchmark is a wall clock, and a wall clock on a shared machine swings.
//! `docs/verification.md` said for as long as the job had existed that a
//! hosted runner swings 20 % between two runs of identical code, *"and a
//! benchmark that fails a pull request on that teaches people to ignore it"*.
//! That sentence was the argument for a band; it was not a measurement of one,
//! and when the measurement was finally taken — fifteen runs of one revision
//! on `ubuntu-latest`, 19 September 2026 — the worst operation swung 88.36 %.
//! A band set on the guess would have failed on the machine four times over.
//!
//! So a baseline entry carries the swing that machine actually showed, over a
//! stated number of runs, and the band is set above it. A band chosen without
//! that measurement is a guess, and this file refuses one: an entry with no
//! `runs` and no `swing_percent` is rejected by [`Baseline::parse`] rather than
//! defaulted, for the reason `pdfa_ledger.tsv` refuses a row with no reason.
//!
//! # Why the band is per operation and not per machine
//!
//! Because the measurement said so. Fifteen dispatches of `bench.yml` on
//! `ubuntu-latest`, 19 September 2026, gave seven very different spreads over
//! one unchanged revision: 22.61 % for "extract a page of text" and 88.36 %
//! for "render text at 150 dpi", with the rest between. One band has to clear
//! the worst of them, so a single machine-wide band would hold the tightest
//! operation to the noisiest one's slack — it would admit a doubling of the
//! text extractor without a word. That is the mistake `BandBelowSwing` refuses,
//! in the other direction, and the memory ratchet already avoids it the same
//! way: `ratchet.json` bands each corpus's peak separately rather than banding
//! all five on the worst.
//!
//! So every operation carries its own measured swing and its own band, and the
//! machine's pair is the summary: its `swing_percent` is the widest any of its
//! operations showed, and its `band_percent` is the ceiling no operation's band
//! may exceed.
//!
//! # Why the baseline is per machine
//!
//! Nanoseconds are not portable and this repository already knows it. The
//! corpus comparator refuses outright when a bar was recorded with different
//! `--fonts`, because *"the two are different measurements"*; a time recorded
//! on one machine and compared against another is the same kind of mistake with
//! a bigger number. So each entry names its machine, the caller says which
//! machine it is on, and a machine with no entry is **reported and not failed**
//! — the shape `ratchet.rs` already uses for a corpus with no recorded bar.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where the committed baseline lives.
///
/// Hand-written, and `"machine"` is deliberately the first key of every entry:
/// [`parse`] cuts the file into entries at that key, so a field written *above*
/// it falls in the previous entry's chunk and is refused as absent. Which is
/// also why this file is not written by `json.rs` — its objects come out
/// ordered by key, `band_percent` sorts above `machine`, and the entry would
/// not read. [`check`] parses the committed file on every `cargo xtask check`,
/// so a reordering fails the build here rather than the job on the runner.
pub const BASELINE_PATH: &str = "crates/tinker-pdf/benches/baseline.json";

/// Where the operations are defined. The baseline names the same ones, and
/// [`check`] holds the two to each other.
pub const BENCH_SOURCE: &str = "crates/tinker-pdf/benches/engine.rs";

/// The weekly job, whose guard counts how many operations reported.
pub const WORKFLOW_PATH: &str = ".github/workflows/bench.yml";

/// One operation's recorded time on one machine.
#[derive(Clone, Debug, PartialEq)]
pub struct Recorded {
    /// Criterion's own id, as it appears in `target/criterion/`.
    pub id: String,
    /// The mean, in nanoseconds, as criterion estimated it — the **fastest**
    /// of the machine's runs, not their average.
    ///
    /// The comparison is one-sided: faster is never a failure, so the figure
    /// worth recording is the best the machine has ever done, and
    /// [`Verdict::moved_percent`] then reads as *how much slower than that*.
    /// Recording a middle figure instead would move the bar up by whatever
    /// the slow half of the fleet contributed and slacken the gate by the
    /// same amount, for no gain.
    pub nanos: f64,
    /// The widest spread this one operation showed over the machine's runs,
    /// as a percentage of its fastest. Measured, never assumed.
    pub swing_percent: f64,
    /// The band this operation must stay inside. Above its own swing, and no
    /// wider than its machine's ceiling.
    pub band_percent: f64,
}

/// What one named machine recorded, and how much it swings.
#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    /// What the machine is called. The caller passes this with `--machine`.
    pub name: String,
    /// How many runs the swing was measured over.
    pub runs: u32,
    /// The widest spread between two runs of identical code on this machine,
    /// as a percentage of the smaller — the worst of its operations'.
    /// Measured, never assumed.
    pub swing_percent: f64,
    /// The ceiling: no operation's band may be wider than this, and it is
    /// above the machine's own widest swing, so the machine's noise cannot
    /// fail a build.
    pub band_percent: f64,
    /// One entry per operation.
    pub operations: Vec<Recorded>,
    /// When the entry was recorded, and by what.
    pub recorded: String,
}

/// Why a baseline was refused.
#[derive(Debug, PartialEq)]
pub enum BaselineError {
    /// The file is not there at all.
    Missing(String),
    /// A field the format requires is absent or unreadable.
    Field {
        /// Which machine's entry, where one is known.
        machine: String,
        /// Which field.
        key: &'static str,
    },
    /// The band is not above the measured swing, so the machine's own noise
    /// would fail a build.
    BandBelowSwing {
        /// Which machine.
        machine: String,
        /// What it swings.
        swing: f64,
        /// What the band admits.
        band: f64,
    },
    /// A field one operation requires is absent or unreadable.
    Operation {
        /// Which machine's entry.
        machine: String,
        /// Which operation.
        id: String,
        /// Which field.
        key: &'static str,
    },
    /// An operation's band is not above that operation's own measured swing.
    OperationBandBelowSwing {
        /// Which machine.
        machine: String,
        /// Which operation.
        id: String,
        /// What that operation swings.
        swing: f64,
        /// What its band admits.
        band: f64,
    },
    /// An operation's band is wider than its machine's ceiling, which would
    /// make the machine's own figure a decoration.
    OperationBandAboveMachine {
        /// Which machine.
        machine: String,
        /// Which operation.
        id: String,
        /// What the operation's band admits.
        band: f64,
        /// What the machine's ceiling admits.
        ceiling: f64,
    },
}

impl core::fmt::Display for BaselineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BaselineError::Missing(path) => write!(f, "no baseline at {path}"),
            BaselineError::Field { machine, key } => write!(
                f,
                "the entry for `{machine}` has no readable `{key}`; a baseline \
                 without one is not a baseline"
            ),
            BaselineError::BandBelowSwing {
                machine,
                swing,
                band,
            } => write!(
                f,
                "`{machine}` swings {swing} % and its band admits {band} %; a \
                 band inside the noise fails on the machine rather than on the \
                 code"
            ),
            BaselineError::Operation { machine, id, key } => write!(
                f,
                "`{machine}` records `{id}` with no readable `{key}`; an \
                 operation whose swing was never measured has no band anybody \
                 can defend"
            ),
            BaselineError::OperationBandBelowSwing {
                machine,
                id,
                swing,
                band,
            } => write!(
                f,
                "`{machine}` records `{id}` swinging {swing} % inside a band of \
                 {band} %; a band inside the noise fails on the machine rather \
                 than on the code"
            ),
            BaselineError::OperationBandAboveMachine {
                machine,
                id,
                band,
                ceiling,
            } => write!(
                f,
                "`{machine}` bands `{id}` at {band} % over a machine ceiling of \
                 {ceiling} %; the ceiling is the widest any operation may be, \
                 so one above it means the machine's own figure is a decoration"
            ),
        }
    }
}

/// Reads the committed baseline.
///
/// Hand-parsed, like every other JSON this crate reads: `xtask/src/json.rs`
/// writes and this reads, and neither is a library.
pub fn parse(text: &str, path: &str) -> Result<Vec<Machine>, BaselineError> {
    if text.trim().is_empty() {
        return Err(BaselineError::Missing(path.to_string()));
    }
    let mut machines = Vec::new();
    for chunk in text.split("\"machine\":").skip(1) {
        let name = string_after(chunk, "").unwrap_or_default();
        if name.is_empty() {
            return Err(BaselineError::Field {
                machine: "(unnamed)".to_string(),
                key: "machine",
            });
        }
        let field = |key: &'static str| -> Result<f64, BaselineError> {
            number_after(chunk, key).ok_or(BaselineError::Field {
                machine: name.clone(),
                key,
            })
        };
        let runs = field("runs")?;
        let swing_percent = field("swing_percent")?;
        let band_percent = field("band_percent")?;
        if band_percent <= swing_percent {
            return Err(BaselineError::BandBelowSwing {
                machine: name.clone(),
                swing: swing_percent,
                band: band_percent,
            });
        }
        let recorded = string_after(chunk, "\"recorded\":").ok_or(BaselineError::Field {
            machine: name.clone(),
            key: "recorded",
        })?;
        let mut operations = Vec::new();
        for entry in chunk.split("\"id\":").skip(1) {
            let id = string_after(entry, "").unwrap_or_default();
            let of = |key: &'static str| -> Result<f64, BaselineError> {
                number_after(entry, key).ok_or(BaselineError::Operation {
                    machine: name.clone(),
                    id: id.clone(),
                    key,
                })
            };
            let nanos = of("nanos")?;
            let op_swing = of("swing_percent")?;
            let op_band = of("band_percent")?;
            if op_band <= op_swing {
                return Err(BaselineError::OperationBandBelowSwing {
                    machine: name.clone(),
                    id,
                    swing: op_swing,
                    band: op_band,
                });
            }
            if op_band > band_percent {
                return Err(BaselineError::OperationBandAboveMachine {
                    machine: name.clone(),
                    id,
                    band: op_band,
                    ceiling: band_percent,
                });
            }
            operations.push(Recorded {
                id,
                nanos,
                swing_percent: op_swing,
                band_percent: op_band,
            });
        }
        if operations.is_empty() {
            return Err(BaselineError::Field {
                machine: name.clone(),
                key: "operations",
            });
        }
        machines.push(Machine {
            name,
            runs: runs as u32,
            swing_percent,
            band_percent,
            operations,
            recorded,
        });
    }
    if machines.is_empty() {
        return Err(BaselineError::Missing(path.to_string()));
    }
    Ok(machines)
}

/// The first string literal after `key`, or after the cursor when `key` is
/// empty.
fn string_after(text: &str, key: &str) -> Option<String> {
    let rest = if key.is_empty() {
        text
    } else {
        &text[text.find(key)? + key.len()..]
    };
    let open = rest.find('"')? + 1;
    let close = rest[open..].find('"')? + open;
    Some(rest[open..close].to_string())
}

/// The first number after `"key":`.
fn number_after(text: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{key}\":");
    let rest = &text[text.find(&needle)? + needle.len()..];
    let digits: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == 'e' || *c == '+')
        .collect();
    digits.parse().ok()
}

/// What criterion measured this run, read out of its own estimates.
///
/// One entry per benchmark directory under `target/criterion`, keyed by the id
/// criterion made the directory name from. `change/` and `report/` are skipped:
/// the first is criterion's own comparison, which this file exists because it
/// cannot fail, and the second is HTML this build does not generate.
pub fn measured(root: &Path) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    collect(root, &mut Vec::new(), &mut out);
    out
}

fn collect(dir: &Path, path: &mut Vec<String>, out: &mut BTreeMap<String, f64>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.path().is_dir() || name == "report" || name == "change" {
            continue;
        }
        if name == "new" {
            if let Some(nanos) = mean_of(&entry.path().join("estimates.json")) {
                out.insert(path.join("/"), nanos);
            }
            continue;
        }
        path.push(name);
        collect(&entry.path(), path, out);
        path.pop();
    }
}

/// The `mean.point_estimate` criterion wrote, in nanoseconds.
fn mean_of(file: &Path) -> Option<f64> {
    let text = std::fs::read_to_string(file).ok()?;
    let mean = &text[text.find("\"mean\"")?..];
    number_after(mean, "point_estimate")
}

/// One operation's verdict.
pub struct Verdict {
    /// The operation.
    pub id: String,
    /// What the baseline says, in nanoseconds.
    pub before: f64,
    /// What this run measured.
    pub now: f64,
    /// The band this operation was held to.
    pub band_percent: f64,
}

impl Verdict {
    /// How far this run moved, as a percentage of the baseline. Positive is
    /// slower.
    #[must_use]
    pub fn moved_percent(&self) -> f64 {
        if self.before <= 0.0 {
            return 0.0;
        }
        (self.now - self.before) / self.before * 100.0
    }
}

/// Compares a run against one machine's committed entry.
///
/// Returns the operations outside the band, and every operation's movement for
/// the report. An operation the baseline does not name is **not** a failure —
/// a new benchmark has no bar, the same way a new corpus has none — but it is
/// returned so the caller can say so.
#[must_use]
pub fn compare(
    machine: &Machine,
    now: &BTreeMap<String, f64>,
) -> (Vec<Verdict>, Vec<Verdict>, Vec<String>) {
    let mut outside = Vec::new();
    let mut inside = Vec::new();
    let mut unrecorded = Vec::new();
    for (id, nanos) in now {
        match machine.operations.iter().find(|op| &op.id == id) {
            Some(recorded) => {
                let verdict = Verdict {
                    id: id.clone(),
                    before: recorded.nanos,
                    now: *nanos,
                    band_percent: recorded.band_percent,
                };
                if verdict.moved_percent() > verdict.band_percent {
                    outside.push(verdict);
                } else {
                    inside.push(verdict);
                }
            }
            None => unrecorded.push(id.clone()),
        }
    }
    (outside, inside, unrecorded)
}

/// Every string literal handed to `.<method>("…"` in `source`.
///
/// Enough of a reader for one file this repository writes: criterion's names
/// are plain literals with no escapes, and a name that grew one would fail
/// [`check`] rather than be read wrongly.
#[must_use]
pub fn literals_passed_to(source: &str, method: &str) -> Vec<String> {
    let needle = format!(".{method}(\"");
    let mut out = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find(&needle) {
        rest = &rest[at + needle.len()..];
        let Some(end) = rest.find('"') else { break };
        out.push(rest[..end].to_string());
        rest = &rest[end..];
    }
    out
}

/// How many criterion lines `bench.yml` insists on seeing.
///
/// The guard is the one thing in that file that carries a count, and a count
/// in a shell script is exactly the kind that drifts: it read `time:+\[` for
/// a fortnight and matched nothing, and it said six for a day after
/// `engine.rs` had seven.
#[must_use]
pub fn guard_count(workflow: &str) -> Option<u32> {
    let rest = &workflow[workflow.find("-eq ")? + "-eq ".len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// What `cargo xtask check` holds the speed ratchet to.
///
/// Three files carry the same list of operations — `engine.rs` defines them,
/// `bench.yml` counts them, and the baseline records a figure for each — and
/// nothing made them agree. Each of the two ways they have already drifted
/// apart failed on the runner, weekly, for as long as nobody read the log:
/// the guard counted six after the seventh operation landed, and the baseline
/// the path names did not exist at all, so the step that promises to "report
/// and not fail for a machine with no entry" exited 1 on every run instead.
///
/// So the agreement is checked where a build can fail on it.
pub fn check(root: &Path) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();

    let source = match std::fs::read_to_string(root.join(BENCH_SOURCE)) {
        Ok(text) => text,
        Err(error) => return Err(vec![format!("cannot read {BENCH_SOURCE}: {error}")]),
    };
    let named = literals_passed_to(&source, "bench_function");
    let groups = literals_passed_to(&source, "benchmark_group");
    if named.is_empty() {
        return Err(vec![format!(
            "{BENCH_SOURCE} names no operation; a benchmark file with nothing \
             in it is the silent pass this whole job exists to stop"
        )]);
    }

    match std::fs::read_to_string(root.join(WORKFLOW_PATH)) {
        Ok(workflow) => match guard_count(&workflow) {
            Some(count) if count as usize == named.len() => {}
            Some(count) => problems.push(format!(
                "{WORKFLOW_PATH} requires {count} criterion lines and {BENCH_SOURCE} \
                 defines {} operations; the job passes on the smaller number and \
                 proves the larger one ran only by accident",
                named.len()
            )),
            None => problems.push(format!(
                "{WORKFLOW_PATH} has no `-eq N` guard; a bench target that compiled \
                 and measured nothing exits 0"
            )),
        },
        Err(error) => problems.push(format!("cannot read {WORKFLOW_PATH}: {error}")),
    }

    let text = std::fs::read_to_string(root.join(BASELINE_PATH)).unwrap_or_default();
    match parse(&text, BASELINE_PATH) {
        Err(error) => problems.push(error.to_string()),
        Ok(machines) => {
            for machine in &machines {
                let widest = machine
                    .operations
                    .iter()
                    .fold(0.0_f64, |worst, op| worst.max(op.swing_percent));
                if (machine.swing_percent - widest).abs() > 0.005 {
                    problems.push(format!(
                        "`{}` says it swings {} % and its widest operation swings \
                         {widest} %; the machine's figure is the worst of its \
                         operations and nothing else",
                        machine.name, machine.swing_percent
                    ));
                }
                let mut recorded = Vec::new();
                for operation in &machine.operations {
                    let (group, leaf) = match operation.id.rsplit_once('/') {
                        Some((group, leaf)) => (Some(group), leaf),
                        None => (None, operation.id.as_str()),
                    };
                    if let Some(group) = group {
                        if !groups.iter().any(|known| known == group) {
                            problems.push(format!(
                                "`{}` records `{}`, whose group `{group}` is not one \
                                 {BENCH_SOURCE} opens",
                                machine.name, operation.id
                            ));
                        }
                    }
                    if named.iter().any(|known| known == leaf) {
                        recorded.push(leaf.to_string());
                    } else {
                        problems.push(format!(
                            "`{}` records `{}`, which {BENCH_SOURCE} does not benchmark",
                            machine.name, operation.id
                        ));
                    }
                }
                for name in &named {
                    if !recorded.iter().any(|seen| seen == name) {
                        problems.push(format!(
                            "`{}` has no figure for `{name}`; a baseline naming fewer \
                             operations than {BENCH_SOURCE} is a ratchet with holes in it",
                            machine.name
                        ));
                    }
                }
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// `cargo xtask bench-check --machine NAME`.
///
/// Reads `target/criterion` and the committed baseline, and fails when an
/// operation is slower than its recorded figure by more than the band.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut machine_name = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--machine" => {
                index += 1;
                machine_name = args.get(index).cloned();
            }
            other => return Err(format!("unknown option `{other}`")),
        }
        index += 1;
    }
    let Some(machine_name) = machine_name else {
        return Err("bench-check needs --machine NAME".to_string());
    };

    let path = root.join(BASELINE_PATH);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let machines = parse(&text, BASELINE_PATH).map_err(|e| e.to_string())?;

    let now = measured(&root.join("target").join("criterion"));
    if now.is_empty() {
        return Err(
            "target/criterion holds no estimates; run `cargo bench -p tinker-pdf` first"
                .to_string(),
        );
    }

    let Some(machine) = machines.iter().find(|m| m.name == machine_name) else {
        // Reported, not failed: a machine with no entry has no bar, which is
        // how `ratchet.rs` treats a corpus with no recorded bar.
        println!("bench-check: no baseline for `{machine_name}`; measured only");
        for (id, nanos) in &now {
            println!("  {id}: {:.3} ms", nanos / 1_000_000.0);
        }
        println!(
            "bench-check: to record this machine, add an entry to {BASELINE_PATH} \
             with its measured swing"
        );
        return Ok(());
    };

    let (outside, inside, unrecorded) = compare(machine, &now);
    println!(
        "bench-check: `{}`, widest measured swing {} %, no band wider than {} % \
         ({} runs, {})",
        machine.name, machine.swing_percent, machine.band_percent, machine.runs, machine.recorded
    );
    for verdict in inside.iter().chain(outside.iter()) {
        println!(
            "  {:+7.2} % of {:6.2} %  {:.3} ms against {:.3} ms  {}",
            verdict.moved_percent(),
            verdict.band_percent,
            verdict.now / 1_000_000.0,
            verdict.before / 1_000_000.0,
            verdict.id
        );
    }
    for id in &unrecorded {
        println!("  new, with no recorded figure: {id}");
    }
    if outside.is_empty() {
        // No "ok" line here: `main`'s `one` prints one for every task that
        // returns, and two of them read as two checks having passed.
        return Ok(());
    }
    let mut message = String::from("slower than the baseline by more than the band:\n");
    for verdict in &outside {
        message.push_str(&format!(
            "  {}: {:.3} ms against {:.3} ms, {:+.2} % of a {} % band\n",
            verdict.id,
            verdict.now / 1_000_000.0,
            verdict.before / 1_000_000.0,
            verdict.moved_percent(),
            verdict.band_percent
        ));
    }
    Err(message)
}

/// The repository root, as the other tasks resolve it.
#[must_use]
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = r#"{
      "machines": [
        {
          "machine": "example",
          "runs": 5,
          "swing_percent": 3.0,
          "band_percent": 12.0,
          "recorded": "5 September 2026",
          "operations": [
            { "id": "open a 3-page document", "nanos": 11921.0,
              "swing_percent": 3.0, "band_percent": 10.0 }
          ]
        }
      ]
    }"#;

    #[test]
    fn a_baseline_reads() {
        let machines = parse(ONE, "x").expect("it parses");
        assert_eq!(machines.len(), 1);
        assert_eq!(machines[0].name, "example");
        assert_eq!(machines[0].runs, 5);
        assert_eq!(machines[0].operations[0].id, "open a 3-page document");
        assert!((machines[0].operations[0].nanos - 11921.0).abs() < 1.0);
        assert!((machines[0].operations[0].swing_percent - 3.0).abs() < 0.001);
        assert!((machines[0].operations[0].band_percent - 10.0).abs() < 0.001);
    }

    /// A band inside the machine's own noise fails on the machine rather than
    /// on the code, so it is refused rather than accepted with a warning.
    #[test]
    fn a_band_inside_the_swing_is_refused() {
        let text = ONE.replace("\"band_percent\": 12.0", "\"band_percent\": 2.0");
        assert_eq!(
            parse(&text, "x"),
            Err(BaselineError::BandBelowSwing {
                machine: "example".to_string(),
                swing: 3.0,
                band: 2.0,
            })
        );
    }

    /// Per operation as well as per machine, since that is where the band that
    /// decides anything now lives.
    #[test]
    fn an_operations_band_inside_its_own_swing_is_refused() {
        let text = ONE.replace("\"band_percent\": 10.0", "\"band_percent\": 1.0");
        assert_eq!(
            parse(&text, "x"),
            Err(BaselineError::OperationBandBelowSwing {
                machine: "example".to_string(),
                id: "open a 3-page document".to_string(),
                swing: 3.0,
                band: 1.0,
            })
        );
    }

    /// The machine's band is the ceiling. An operation banded above it would
    /// make the machine's figure a decoration, and the printed summary a lie.
    #[test]
    fn an_operation_banded_above_its_machines_ceiling_is_refused() {
        let text = ONE.replace("\"band_percent\": 10.0", "\"band_percent\": 40.0");
        assert_eq!(
            parse(&text, "x"),
            Err(BaselineError::OperationBandAboveMachine {
                machine: "example".to_string(),
                id: "open a 3-page document".to_string(),
                band: 40.0,
                ceiling: 12.0,
            })
        );
    }

    /// The swing is the thing the roadmap row asks to be measured first, so an
    /// entry without it is not a baseline. Three shapes, because a reader that
    /// validated only the first field would stop looking.
    #[test]
    fn an_entry_missing_a_measurement_is_refused() {
        for key in ["runs", "swing_percent", "band_percent"] {
            let text = ONE.replace(&format!("\"{key}\""), "\"unused\"");
            assert_eq!(
                parse(&text, "x"),
                Err(BaselineError::Field {
                    machine: "example".to_string(),
                    key,
                }),
                "removing {key} must refuse the entry"
            );
        }
    }

    /// And an operation without one is not an operation. The same three
    /// shapes, one level down, because an operation is where the band that
    /// decides anything is now written.
    #[test]
    fn an_operation_missing_a_measurement_is_refused() {
        let around = |operation: &str| {
            format!(
                "{{ \"machines\": [ {{ \"machine\": \"example\", \"runs\": 5, \
                 \"swing_percent\": 3.0, \"band_percent\": 12.0, \
                 \"recorded\": \"5 September 2026\", \
                 \"operations\": [ {operation} ] }} ] }}"
            )
        };
        for (key, operation) in [
            (
                "nanos",
                r#"{ "id": "open it", "swing_percent": 3.0, "band_percent": 10.0 }"#,
            ),
            (
                "swing_percent",
                r#"{ "id": "open it", "nanos": 11921.0, "band_percent": 10.0 }"#,
            ),
            (
                "band_percent",
                r#"{ "id": "open it", "nanos": 11921.0, "swing_percent": 3.0 }"#,
            ),
        ] {
            assert_eq!(
                parse(&around(operation), "x"),
                Err(BaselineError::Operation {
                    machine: "example".to_string(),
                    id: "open it".to_string(),
                    key,
                }),
                "an operation with no {key} must refuse the entry"
            );
        }
    }

    #[test]
    fn an_empty_baseline_is_missing_rather_than_empty() {
        assert_eq!(parse("", "p"), Err(BaselineError::Missing("p".to_string())));
        assert_eq!(
            parse("{\"machines\":[]}", "p"),
            Err(BaselineError::Missing("p".to_string()))
        );
    }

    /// The comparison is one-sided on purpose: faster is never a failure, and
    /// a run that got faster is reported so somebody can re-record it.
    #[test]
    fn faster_is_never_a_failure_and_slower_past_the_band_always_is() {
        let machine = parse(ONE, "x").expect("parses").remove(0);
        let mut now = BTreeMap::new();
        now.insert("open a 3-page document".to_string(), 5000.0);
        let (outside, inside, _) = compare(&machine, &now);
        assert!(outside.is_empty(), "twice as fast is not a regression");
        assert_eq!(inside.len(), 1);

        now.insert("open a 3-page document".to_string(), 11921.0 * 1.05);
        let (outside, inside, _) = compare(&machine, &now);
        assert!(outside.is_empty(), "5 % is inside a 10 % band");
        assert_eq!(inside.len(), 1);

        now.insert("open a 3-page document".to_string(), 11921.0 * 1.11);
        let (outside, _, _) = compare(&machine, &now);
        assert_eq!(outside.len(), 1, "11 % is outside a 10 % band");
        assert!((outside[0].moved_percent() - 11.0).abs() < 0.01);
    }

    /// A benchmark the baseline does not name is new, and a new benchmark has
    /// no bar — the same answer `ratchet.rs` gives a corpus with no recorded
    /// row, and for the same reason.
    #[test]
    fn a_benchmark_with_no_recorded_figure_is_reported_and_not_failed() {
        let machine = parse(ONE, "x").expect("parses").remove(0);
        let mut now = BTreeMap::new();
        now.insert("something new".to_string(), 1.0);
        let (outside, inside, unrecorded) = compare(&machine, &now);
        assert!(outside.is_empty());
        assert!(inside.is_empty());
        assert_eq!(unrecorded, vec!["something new".to_string()]);
    }

    /// Criterion's own numbers, read out of its own file rather than timed
    /// again here.
    /// The one that catches drift in the tree rather than in a fixture: the
    /// committed baseline, the operations `engine.rs` defines and the count
    /// `bench.yml` guards, held to each other as they stand.
    #[test]
    fn the_committed_baseline_agrees_with_the_tree() {
        if let Err(problems) = check(&repo_root()) {
            panic!("{}", problems.join("\n"));
        }
    }

    /// Every band in the committed file, pushed against from both sides: a
    /// run one point past an operation's band fails on that operation and on
    /// nothing else, and a run one point inside it passes.
    ///
    /// A band nobody has ever pushed past is a number nobody has checked, and
    /// this file's whole reason for existing is that criterion's own
    /// comparison never fails. Seven operations, seven bands, and the figures
    /// are the committed ones rather than a fixture's, so a band edited to a
    /// number that cannot fire is caught here.
    #[test]
    fn every_committed_band_fires_at_its_own_edge_and_not_before() {
        let text =
            std::fs::read_to_string(repo_root().join(BASELINE_PATH)).expect("the committed file");
        let machines = parse(&text, BASELINE_PATH).expect("it parses");
        for machine in &machines {
            for operation in &machine.operations {
                let at = |factor: f64| {
                    let mut now: BTreeMap<String, f64> = machine
                        .operations
                        .iter()
                        .map(|other| (other.id.clone(), other.nanos))
                        .collect();
                    now.insert(operation.id.clone(), operation.nanos * factor);
                    compare(machine, &now)
                };
                let (outside, inside, unrecorded) =
                    at(1.0 + (operation.band_percent - 1.0) / 100.0);
                assert!(
                    outside.is_empty(),
                    "`{}` one point inside its own band must pass",
                    operation.id
                );
                assert_eq!(inside.len(), machine.operations.len());
                assert!(unrecorded.is_empty());

                let (outside, _, _) = at(1.0 + (operation.band_percent + 1.0) / 100.0);
                assert_eq!(
                    outside.len(),
                    1,
                    "`{}` one point past its own band must fail, and alone",
                    operation.id
                );
                assert_eq!(outside[0].id, operation.id);
            }
        }
    }

    #[test]
    fn criterions_names_are_read_out_of_the_benchmark_file() {
        let source = r#"
            let mut slow = c.benchmark_group("shading");
            slow.bench_function("render a full-page axial shading at 150 dpi", |b| {});
            c.bench_function("open a 3-page document", |b| {});
        "#;
        assert_eq!(
            literals_passed_to(source, "bench_function"),
            vec![
                "render a full-page axial shading at 150 dpi".to_string(),
                "open a 3-page document".to_string(),
            ]
        );
        assert_eq!(
            literals_passed_to(source, "benchmark_group"),
            vec!["shading".to_string()]
        );
    }

    #[test]
    fn the_guard_count_is_read_out_of_the_workflow() {
        assert_eq!(
            guard_count("          test \"$(grep -Ec 'time: +\\[' bench.log)\" -eq 7\n"),
            Some(7)
        );
        assert_eq!(guard_count("cargo bench -p tinker-pdf\n"), None);
    }

    /// A root with the three files in it, so the disagreements can be shown
    /// one at a time.
    fn staged(tag: &str, baseline: &str, guard: u32) -> PathBuf {
        let root = std::env::temp_dir().join(format!("tinker-baseline-{tag}"));
        std::fs::remove_dir_all(&root).ok();
        let benches = root.join("crates").join("tinker-pdf").join("benches");
        std::fs::create_dir_all(&benches).expect("a directory");
        std::fs::write(
            benches.join("engine.rs"),
            "let mut slow = c.benchmark_group(\"shading\");\n\
             slow.bench_function(\"render a full-page axial shading at 150 dpi\", |b| {});\n\
             c.bench_function(\"open a 3-page document\", |b| {});\n",
        )
        .expect("a file");
        std::fs::write(benches.join("baseline.json"), baseline).expect("a file");
        let workflows = root.join(".github").join("workflows");
        std::fs::create_dir_all(&workflows).expect("a directory");
        std::fs::write(
            workflows.join("bench.yml"),
            format!("test \"$(grep -Ec 'time: +\\[' bench.log)\" -eq {guard}\n"),
        )
        .expect("a file");
        root
    }

    const TWO: &str = r#"{
      "machines": [
        {
          "machine": "example",
          "runs": 5,
          "swing_percent": 6.0,
          "band_percent": 12.0,
          "recorded": "5 September 2026",
          "operations": [
            { "id": "open a 3-page document", "nanos": 11921.0,
              "swing_percent": 3.0, "band_percent": 10.0 },
            { "id": "shading/render a full-page axial shading at 150 dpi",
              "nanos": 164850000.0, "swing_percent": 6.0, "band_percent": 11.0 }
          ]
        }
      ]
    }"#;

    #[test]
    fn three_files_carrying_one_list_are_held_to_each_other() {
        let root = staged("agrees", TWO, 2);
        assert_eq!(check(&root), Ok(()));
        std::fs::remove_dir_all(&root).ok();
    }

    /// The drift that ran weekly for a fortnight: `engine.rs` gained an
    /// operation and the guard still counted the old number, so the job
    /// passed on the smaller one.
    #[test]
    fn a_guard_counting_fewer_operations_than_the_file_defines_is_refused() {
        let root = staged("guard", TWO, 1);
        let problems = check(&root).expect_err("the counts disagree");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("requires 1 criterion lines"),
            "{problems:?}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// A baseline with a hole in it is a ratchet with a hole in it: the
    /// operation it does not name can regress without limit, and
    /// `compare` reports it as new rather than failing.
    #[test]
    fn a_baseline_short_of_an_operation_is_refused() {
        let text = TWO.replace(
            "{ \"id\": \"open a 3-page document\", \"nanos\": 11921.0,\n              \
             \"swing_percent\": 3.0, \"band_percent\": 10.0 },",
            "",
        );
        let root = staged("short", &text, 2);
        let problems = check(&root).expect_err("one operation has no figure");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("no figure for `open a 3-page document`"),
            "{problems:?}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_baseline_naming_an_operation_nobody_benchmarks_is_refused() {
        let text = TWO.replace("open a 3-page document", "open a document nobody opens");
        let root = staged("stale", &text, 2);
        let problems = check(&root).expect_err("the name is not in the file");
        assert!(
            problems.iter().any(|p| p.contains("does not benchmark")),
            "{problems:?}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// The failure this row was opened on: the path named a file that was not
    /// there, so the step that promises to report and not fail exited 1 on
    /// every run of the weekly job.
    #[test]
    fn a_baseline_that_is_not_there_is_refused_by_name() {
        let root = staged("absent", "", 2);
        std::fs::remove_file(
            root.join("crates")
                .join("tinker-pdf")
                .join("benches")
                .join("baseline.json"),
        )
        .ok();
        let problems = check(&root).expect_err("there is no baseline");
        assert!(
            problems.iter().any(|p| p.contains(BASELINE_PATH)),
            "{problems:?}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_mean_comes_out_of_criterions_estimates() {
        let dir = std::env::temp_dir().join("tinker-bench-check-test");
        let bench = dir.join("open a 3-page document").join("new");
        std::fs::create_dir_all(&bench).expect("a directory");
        std::fs::write(
            bench.join("estimates.json"),
            r#"{"mean":{"confidence_interval":{"confidence_level":0.95,
               "lower_bound":11892.0,"upper_bound":11963.0},
               "point_estimate":11921.5,"standard_error":18.0}}"#,
        )
        .expect("a file");
        let found = measured(&dir);
        assert_eq!(found.len(), 1);
        assert!((found["open a 3-page document"] - 11921.5).abs() < 0.01);
        std::fs::remove_dir_all(&dir).ok();
    }
}
