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
//! `docs/verification.md` has said for as long as the job has existed that a
//! hosted runner swings 20 % between two runs of identical code, *"and a
//! benchmark that fails a pull request on that teaches people to ignore it"*.
//! That sentence is the argument for a band; it is not a measurement of one.
//!
//! So a baseline entry carries the swing that machine actually showed, over a
//! stated number of runs, and the band is set above it. A band chosen without
//! that measurement is a guess, and this file refuses one: an entry with no
//! `runs` and no `swing_percent` is rejected by [`Baseline::parse`] rather than
//! defaulted, for the reason `pdfa_ledger.tsv` refuses a row with no reason.
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
pub const BASELINE_PATH: &str = "crates/tinker-pdf/benches/baseline.json";

/// One operation's recorded time on one machine.
#[derive(Clone, Debug, PartialEq)]
pub struct Recorded {
    /// Criterion's own id, as it appears in `target/criterion/`.
    pub id: String,
    /// The mean, in nanoseconds, as criterion estimated it.
    pub nanos: f64,
}

/// What one named machine recorded, and how much it swings.
#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    /// What the machine is called. The caller passes this with `--machine`.
    pub name: String,
    /// How many runs the swing was measured over.
    pub runs: u32,
    /// The widest spread between two runs of identical code on this machine,
    /// as a percentage of the smaller. Measured, never assumed.
    pub swing_percent: f64,
    /// The band a run must stay inside, as a percentage. Set above the swing,
    /// so the machine's own noise cannot fail a build.
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
            let Some(nanos) = number_after(entry, "nanos") else {
                return Err(BaselineError::Field {
                    machine: name.clone(),
                    key: "nanos",
                });
            };
            operations.push(Recorded { id, nanos });
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
                };
                if verdict.moved_percent() > machine.band_percent {
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
        "bench-check: `{}`, band {} % over a measured swing of {} % ({} runs, {})",
        machine.name, machine.band_percent, machine.swing_percent, machine.runs, machine.recorded
    );
    for verdict in inside.iter().chain(outside.iter()) {
        println!(
            "  {:+7.2} %  {:.3} ms against {:.3} ms  {}",
            verdict.moved_percent(),
            verdict.now / 1_000_000.0,
            verdict.before / 1_000_000.0,
            verdict.id
        );
    }
    for id in &unrecorded {
        println!("  new, with no recorded figure: {id}");
    }
    if outside.is_empty() {
        println!("bench-check: ok");
        return Ok(());
    }
    let mut message = String::from("slower than the baseline by more than the band:\n");
    for verdict in &outside {
        message.push_str(&format!(
            "  {}: {:.3} ms against {:.3} ms, {:+.2} %\n",
            verdict.id,
            verdict.now / 1_000_000.0,
            verdict.before / 1_000_000.0,
            verdict.moved_percent()
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
          "band_percent": 10.0,
          "recorded": "5 September 2026",
          "operations": [
            { "id": "open a 3-page document", "nanos": 11921.0 }
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
    }

    /// A band inside the machine's own noise fails on the machine rather than
    /// on the code, so it is refused rather than accepted with a warning.
    #[test]
    fn a_band_inside_the_swing_is_refused() {
        let text = ONE.replace("\"band_percent\": 10.0", "\"band_percent\": 2.0");
        assert_eq!(
            parse(&text, "x"),
            Err(BaselineError::BandBelowSwing {
                machine: "example".to_string(),
                swing: 3.0,
                band: 2.0,
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
        let text = ONE.replace("\"nanos\"", "\"unused\"");
        assert_eq!(
            parse(&text, "x"),
            Err(BaselineError::Field {
                machine: "example".to_string(),
                key: "nanos",
            })
        );
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
