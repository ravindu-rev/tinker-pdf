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
//!
//! # Why one list a group
//!
//! The benchmark set is one file a group, `benches/<group>.rs`, each its own
//! `[[bench]]`, and `bench.yml` runs one job a group (MS-04). So the baseline
//! lists each group's operations ahead of its machines, in the order the
//! group's file declares them, each with its unit, what one iteration covers
//! and how many calls it makes ([`Listed`]). A machine entry names the
//! operations it has no figure for yet ([`Machine::unmeasured`]): before the
//! lists, an entry short of any operation was refused outright, so no
//! operation could join the set until the whole set was measured again, and a
//! hole the entry names is one nobody can miss. [`check`] holds the
//! `[[bench]]` entries, each file's names, its list, `bench.yml`'s count for
//! the group and every machine entry to each other.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where the committed baseline lives.
///
/// Hand-written, and key order is load-bearing. The lists come first and the
/// machines after them: [`parse`] reads machines only after `"machines":` and
/// [`groups`] reads lists only before it, so no list entry can fall into a
/// machine's chunk. Within the machines, `"machine"` is deliberately the first
/// key of every entry: [`parse`] cuts them into entries at that key, so a
/// field written *above* it falls in the previous entry's chunk and is refused
/// as absent; an entry's `"unmeasured"` comes before its `"operations"`, which
/// run to the entry's end. Within the lists, `"group"` and `"name"` come first
/// for the same reason. Which is also why this file is not written by
/// `json.rs` — its objects come out ordered by key, `band_percent` sorts above
/// `machine`, and the entry would not read. [`check`] parses the committed
/// file on every `cargo xtask check`, so a reordering fails the build here
/// rather than the job on the runner.
pub const BASELINE_PATH: &str = "crates/tinker-pdf/benches/baseline.json";

/// Where the group files live: `<group>.rs` for each group, beside the
/// `support/` module they share.
pub const BENCH_DIR: &str = "crates/tinker-pdf/benches";

/// The manifest that declares each group file a `[[bench]]`.
pub const BENCH_MANIFEST: &str = "crates/tinker-pdf/Cargo.toml";

/// The weekly job, one entry of its matrix a group, whose guard counts how
/// many of the group's operations reported.
pub const WORKFLOW_PATH: &str = ".github/workflows/bench.yml";

/// The guard each job of the matrix runs, comparing the criterion lines it
/// counted with its own entry's `count`.
pub const GUARD: &str = "-eq ${{ matrix.count }}";

/// The ten characters criterion 0.5.1 writes as `_` in each part of an
/// operation's directory (`make_filename_safe`, its `src/report.rs`).
pub const REWRITTEN: [char; 10] = ['?', '"', '/', '\\', '*', '<', '>', ':', '|', '^'];

/// The longest part, in bytes, that criterion 0.5.1 keeps whole: its
/// `MAX_DIRECTORY_NAME_LEN`.
pub const LONGEST_PART: usize = 64;

/// What one iteration of an operation processes: the unit its throughput is
/// given in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// Pages read, written or laid out.
    Pages,
    /// Pixels: a render's bitmap's, or a decoded image's.
    Pixels,
    /// Bytes.
    Bytes,
    /// Glyph outlines.
    Outlines,
    /// None of these: the operation reports milliseconds alone.
    None,
}

impl Unit {
    /// The unit `word` names in the baseline, if it is one of the five.
    #[must_use]
    pub fn named(word: &str) -> Option<Unit> {
        Some(match word {
            "pages" => Unit::Pages,
            "pixels" => Unit::Pixels,
            "bytes" => Unit::Bytes,
            "outlines" => Unit::Outlines,
            "none" => Unit::None,
            _ => return None,
        })
    }
}

/// One operation of a group's list.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    /// Its name inside the group, as its file passes it to `operation`.
    pub name: String,
    /// What an iteration processes.
    pub unit: Unit,
    /// How much of `unit` one iteration processes, every call of it
    /// included: at least one.
    pub per_iteration: u64,
    /// How many calls one iteration makes: at least one.
    pub calls: u64,
}

/// One group's list of operations, in the order its file declares them.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// The group, as its file opens it and its `[[bench]]` names it.
    pub name: String,
    /// Its operations.
    pub operations: Vec<Listed>,
}

impl Group {
    /// Criterion's id for one of this group's operations, as a machine entry
    /// records it: `<group>/<name>`.
    #[must_use]
    pub fn id(&self, name: &str) -> String {
        format!("{}/{name}", self.name)
    }
}

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
    /// The operations the lists hold that this machine has no figure for
    /// yet, by id: named, so the hole is never silent.
    pub unmeasured: Vec<String>,
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
    /// A field of a machine entry is where the reader cannot take it for
    /// that entry's.
    Misplaced {
        /// Which machine's entry.
        machine: String,
        /// Which field.
        key: &'static str,
    },
    /// The file lists no group ahead of its machines.
    NoGroups(String),
    /// A group's list has no name, or holds no operation.
    Group {
        /// Which group, where one is known.
        group: String,
        /// Which field.
        key: &'static str,
    },
    /// A field one listed operation requires is absent, unreadable or
    /// outside the format.
    Listed {
        /// Which group's list.
        group: String,
        /// Which operation.
        name: String,
        /// Which field.
        key: &'static str,
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
            BaselineError::Misplaced { machine, key } => write!(
                f,
                "the entry for `{machine}` puts `{key}` after its `operations`; \
                 the reader takes an entry's operations to its end, so the ids \
                 it has not measured come first"
            ),
            BaselineError::NoGroups(path) => write!(
                f,
                "{path} lists no group ahead of its machines; a baseline with no \
                 lists holds nothing to the bench files"
            ),
            BaselineError::Group { group, key } => match *key {
                "operations" => write!(
                    f,
                    "the `{group}` list holds no operation; a group that measures \
                     nothing is the silent pass this whole job exists to stop"
                ),
                _ => write!(f, "a list in the baseline has no readable `{key}`"),
            },
            BaselineError::Listed { group, name, key } => match *key {
                "unit" => write!(
                    f,
                    "the `{group}` list gives `{name}` no unit of the five: pages, \
                     pixels, bytes, outlines or none"
                ),
                "per_iteration" => write!(
                    f,
                    "the `{group}` list gives `{name}` no `per_iteration` of one or \
                     more; it is how much of its unit one iteration processes"
                ),
                "calls" => write!(
                    f,
                    "the `{group}` list gives `{name}` no `calls` of one or more; it \
                     is how many calls one iteration makes"
                ),
                _ => write!(
                    f,
                    "the `{group}` list holds an operation with no readable `{key}`"
                ),
            },
        }
    }
}

/// The key the machine entries follow, and which the lists come before.
const MACHINES: &str = "\"machines\":";

/// Reads the committed baseline's machine entries.
///
/// Hand-parsed, like every other JSON this crate reads: `xtask/src/json.rs`
/// writes and this reads, and neither is a library. Only what follows
/// `"machines":` is read, so nothing in the lists ahead of it can be taken
/// for a machine's field.
pub fn parse(text: &str, path: &str) -> Result<Vec<Machine>, BaselineError> {
    if text.trim().is_empty() {
        return Err(BaselineError::Missing(path.to_string()));
    }
    let Some(at) = text.find(MACHINES) else {
        return Err(BaselineError::Missing(path.to_string()));
    };
    let mut machines = Vec::new();
    for chunk in text[at + MACHINES.len()..].split("\"machine\":").skip(1) {
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
        // The operations run to the entry's end, so whatever names the
        // entry's holes comes ahead of them, and is refused after them.
        let (head, body) = chunk.split_once("\"operations\":").unwrap_or((chunk, ""));
        if body.contains("\"unmeasured\":") {
            return Err(BaselineError::Misplaced {
                machine: name.clone(),
                key: "unmeasured",
            });
        }
        let unmeasured = if head.contains("\"unmeasured\":") {
            strings_after(head, "\"unmeasured\":").ok_or(BaselineError::Field {
                machine: name.clone(),
                key: "unmeasured",
            })?
        } else {
            Vec::new()
        };
        let mut operations = Vec::new();
        for entry in body.split("\"id\":").skip(1) {
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
            unmeasured,
            recorded,
        });
    }
    if machines.is_empty() {
        return Err(BaselineError::Missing(path.to_string()));
    }
    Ok(machines)
}

/// Reads the committed baseline's lists: one a group, ahead of its machines.
///
/// Only what lies between `"groups":` and `"machines":` is read, and each
/// list is cut at `"group":` and each operation at `"name":`, so those keys
/// come first in theirs, as `"machine"` does in a machine entry.
pub fn groups(text: &str, path: &str) -> Result<Vec<Group>, BaselineError> {
    let end = text.find(MACHINES).unwrap_or(text.len());
    let Some(start) = text[..end].find("\"groups\":") else {
        return Err(BaselineError::NoGroups(path.to_string()));
    };
    let mut groups = Vec::new();
    for chunk in text[start + "\"groups\":".len()..end]
        .split("\"group\":")
        .skip(1)
    {
        let group = string_after(chunk, "").unwrap_or_default();
        if group.is_empty() {
            return Err(BaselineError::Group {
                group: "(unnamed)".to_string(),
                key: "group",
            });
        }
        let mut operations = Vec::new();
        for entry in chunk.split("\"name\":").skip(1) {
            let name = string_after(entry, "").unwrap_or_default();
            let refused = |key: &'static str| BaselineError::Listed {
                group: group.clone(),
                name: name.clone(),
                key,
            };
            if name.is_empty() {
                return Err(refused("name"));
            }
            let unit = string_after(entry, "\"unit\":")
                .and_then(|word| Unit::named(&word))
                .ok_or_else(|| refused("unit"))?;
            let at_least_one = |key: &'static str| {
                integer_after(entry, key)
                    .filter(|n| *n >= 1)
                    .ok_or_else(|| refused(key))
            };
            let per_iteration = at_least_one("per_iteration")?;
            let calls = at_least_one("calls")?;
            operations.push(Listed {
                name,
                unit,
                per_iteration,
                calls,
            });
        }
        if operations.is_empty() {
            return Err(BaselineError::Group {
                group,
                key: "operations",
            });
        }
        groups.push(Group {
            name: group,
            operations,
        });
    }
    if groups.is_empty() {
        return Err(BaselineError::NoGroups(path.to_string()));
    }
    Ok(groups)
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

/// The string literals of the array after `key`, or `None` when no array of
/// them follows it.
fn strings_after(text: &str, key: &str) -> Option<Vec<String>> {
    let mut rest = text[text.find(key)? + key.len()..]
        .trim_start()
        .strip_prefix('[')?;
    let mut out = Vec::new();
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        if rest.starts_with(']') {
            return Some(out);
        }
        let literal = rest.strip_prefix('"')?;
        let close = literal.find('"')?;
        out.push(literal[..close].to_string());
        rest = &literal[close + 1..];
    }
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

/// The whole number after `"key":`, or `None` when what follows is not one.
fn integer_after(text: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\":");
    let rest = text[text.find(&needle)? + needle.len()..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    if matches!(rest[end..].chars().next(), Some('.' | 'e' | 'E')) {
        return None;
    }
    rest[..end].parse().ok()
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

/// Every string literal handed to `.<method>(` in `source`, whitespace
/// between the two allowed, since rustfmt moves the arguments of a call that
/// does not fit on one line onto lines of their own.
///
/// Enough of a reader for files this repository writes: criterion's names
/// are plain literals with no escapes, and a name that grew one is read only
/// as far as its escape, which [`rewritten`] then refuses.
#[must_use]
pub fn literals_passed_to(source: &str, method: &str) -> Vec<String> {
    literals_after(source, &format!(".{method}("))
}

/// Every string literal that follows `prefix` in `source`, whitespace between
/// the two allowed: the reader [`literals_passed_to`] is, for a call that is
/// no method's, such as [`GROUP_OPENED`].
#[must_use]
pub fn literals_after(source: &str, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find(prefix) {
        rest = rest[at + prefix.len()..].trim_start();
        let Some(literal) = rest.strip_prefix('"') else {
            continue;
        };
        let Some(end) = literal.find('"') else { break };
        out.push(literal[..end].to_string());
        rest = &literal[end..];
    }
    out
}

/// How a group file opens its group: the harness's own, whose operations run
/// under criterion and in count mode alike (`benches/support/mod.rs`).
pub const GROUP_OPENED: &str = "Group::new(c, ";

/// The groups whose files are frozen, and so keep criterion's own
/// `benchmark_group` and `bench_function` and run under criterion alone.
///
/// `calibration` is the one: its file is the same input and call as the text
/// group's `text of 100 dense pages`, kept apart so that nothing a later row
/// adds can move it, and a change to it re-states every calibrated target,
/// which is the owner's (H-77, `docs/design/performance.md`). Count mode
/// counts that input and call as the text group's operation.
pub const FROZEN: &[&str] = &["calibration"];

/// Each entry of `bench.yml`'s matrix: its `group`, and the `count` under it
/// — how many criterion lines that group's job insists on seeing.
///
/// The guard is the one thing in that file that carries a count, and a count
/// in a shell script is exactly the kind that drifts: it read `time:+\[` for
/// a fortnight and matched nothing, and it said six for a day after
/// `engine.rs` had seven. One job a group now, so one count a group.
#[must_use]
pub fn guard_counts(workflow: &str) -> Vec<(String, Option<u32>)> {
    let mut entries: Vec<(String, Option<u32>)> = Vec::new();
    for line in workflow.lines().map(str::trim) {
        if let Some(group) = line.strip_prefix("- group:") {
            entries.push((group.trim().to_string(), None));
        } else if let Some(count) = line.strip_prefix("count:") {
            if let Some(entry) = entries.last_mut() {
                if entry.1.is_none() {
                    entry.1 = count.trim().parse().ok();
                }
            }
        }
    }
    entries
}

/// The name of every `[[bench]]` a manifest declares, in its order.
#[must_use]
pub fn bench_targets(manifest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_bench = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_bench = line == "[[bench]]";
        } else if let Some((key, value)) = line.split_once('=') {
            if in_bench && key.trim() == "name" {
                if let Some(name) = string_after(value, "") {
                    out.push(name);
                }
            }
        }
    }
    out
}

/// Why criterion would not record an operation, or a group, under `name`
/// itself — so the baseline's figure for it would never be read — or why the
/// name breaks a rule the set keeps; `None` when it is a name the set may use.
///
/// Criterion 0.5.1 makes each part of an operation's directory filename-safe:
/// [`REWRITTEN`]'s ten characters become `_` and the part is cut at
/// [`LONGEST_PART`] bytes. A name either rule touches is recorded under a key
/// the baseline does not hold, reported as new and never held to its band. A
/// name also keeps to printable ASCII, so that its length in bytes, which
/// criterion cuts, is its length in characters.
#[must_use]
pub fn rewritten(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("it is empty".to_string());
    }
    if let Some(c) = name.chars().find(|c| REWRITTEN.contains(c)) {
        return Some(format!(
            "criterion writes `{c}` as `_`, so it records the name under another key"
        ));
    }
    if let Some(c) = name.chars().find(|c| !(c.is_ascii_graphic() || *c == ' ')) {
        return Some(format!(
            "{c:?} is outside printable ASCII, which every name keeps to so that its \
             length in bytes, which criterion cuts, is its length in characters"
        ));
    }
    if name.len() > LONGEST_PART {
        return Some(format!(
            "it is {} bytes and criterion cuts a part at {LONGEST_PART}, so it records \
             the name under another key",
            name.len()
        ));
    }
    None
}

/// The stems of the `.rs` files directly in `dir`, sorted: the group files,
/// since the module they share is a directory.
fn group_files(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file() && path.extension().is_some_and(|e| e == "rs"))
                .filter_map(|path| Some(path.file_stem()?.to_str()?.to_string()))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Each name `names` holds more than once, once.
fn twice(names: &[String]) -> Vec<&String> {
    let mut out: Vec<&String> = Vec::new();
    for (index, name) in names.iter().enumerate() {
        if names[..index].contains(name) && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// `names` with each name after its first left out.
fn once(names: &[String]) -> Vec<&String> {
    let mut out: Vec<&String> = Vec::new();
    for name in names {
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// What `cargo xtask check` holds the speed ratchet to.
///
/// The files carry one list of operations a group — each `[[bench]]` names a
/// group file, the file defines the group's operations, `bench.yml`'s matrix
/// counts them, and the baseline lists them and records a figure for each
/// or names it unmeasured — and nothing else makes them agree. Each of the
/// two ways they had drifted apart while they were one file failed on the
/// runner, weekly, for as long as nobody read the log: the guard counted six
/// after the seventh operation landed, and the baseline the path names did
/// not exist at all, so the step that promises to "report and not fail for a
/// machine with no entry" exited 1 on every run instead.
///
/// So the agreement is checked where a build can fail on it.
pub fn check(root: &Path) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();

    let manifest = match std::fs::read_to_string(root.join(BENCH_MANIFEST)) {
        Ok(text) => text,
        Err(error) => return Err(vec![format!("cannot read {BENCH_MANIFEST}: {error}")]),
    };
    let targets = bench_targets(&manifest);
    if targets.is_empty() {
        return Err(vec![format!(
            "{BENCH_MANIFEST} declares no `[[bench]]`; a speed ratchet over no \
             benchmark is the silent pass this whole job exists to stop"
        )]);
    }

    // Each `[[bench]]` is a group file that opens its group and names its
    // operations, and each group file is a `[[bench]]`.
    let mut defined: Vec<(String, String, Vec<String>)> = Vec::new();
    for target in &targets {
        let file = format!("{BENCH_DIR}/{target}.rs");
        let source = match std::fs::read_to_string(root.join(&file)) {
            Ok(text) => text,
            Err(error) => {
                problems.push(format!("cannot read {file}: {error}"));
                continue;
            }
        };
        // A frozen file keeps criterion's own calls; every other one opens
        // the harness's group and declares each operation through it, since
        // count mode runs only what `Group::operation` declares.
        let frozen = FROZEN.contains(&target.as_str());
        let opened = if frozen {
            literals_passed_to(&source, "benchmark_group")
        } else {
            literals_after(&source, GROUP_OPENED)
        };
        if opened.is_empty() {
            problems.push(format!(
                "`{file}` opens no group; a group file opens one group, named for \
                 its group, so every id it records is `{target}/<name>`"
            ));
        }
        for other in once(&opened) {
            if other != target {
                problems.push(format!(
                    "`{file}` opens `{other}`; a group file opens one group, named \
                     for its group, so every id it records is `{target}/<name>`"
                ));
            }
        }
        if !frozen {
            for call in ["benchmark_group", "bench_function"] {
                if source.contains(&format!(".{call}(")) {
                    problems.push(format!(
                        "`{file}` calls criterion's `{call}` itself, and count mode \
                         runs only what `Group::operation` declares: open the group \
                         with `{GROUP_OPENED}\"{target}\")` and declare each \
                         operation through `operation`"
                    ));
                }
            }
        }
        let names = if frozen {
            literals_passed_to(&source, "bench_function")
        } else {
            literals_passed_to(&source, "operation")
        };
        if names.is_empty() {
            problems.push(format!(
                "`{file}` names no operation; a benchmark file with nothing in it \
                 is the silent pass this whole job exists to stop"
            ));
        }
        for name in twice(&names) {
            problems.push(format!(
                "`{file}` benchmarks `{name}` twice, and criterion records the \
                 second as `{name}_2`"
            ));
        }
        defined.push((target.clone(), file, names));
    }
    for stem in group_files(&root.join(BENCH_DIR)) {
        if !targets.contains(&stem) {
            problems.push(format!(
                "`{BENCH_DIR}/{stem}.rs` is no `[[bench]]` in {BENCH_MANIFEST}; \
                 cargo would build it under its own test harness, whose `main` \
                 criterion's cannot share, and no job would measure it"
            ));
        }
    }

    let text = std::fs::read_to_string(root.join(BASELINE_PATH)).unwrap_or_default();
    let (lists, machines) = if text.trim().is_empty() {
        problems.push(BaselineError::Missing(BASELINE_PATH.to_string()).to_string());
        (None, None)
    } else {
        let lists = groups(&text, BASELINE_PATH)
            .map_err(|error| problems.push(error.to_string()))
            .ok();
        let machines = parse(&text, BASELINE_PATH)
            .map_err(|error| problems.push(error.to_string()))
            .ok();
        (lists, machines)
    };

    // Each group's list holds its file's names, in its file's order, each a
    // name criterion records as written.
    if let Some(lists) = &lists {
        for (index, list) in lists.iter().enumerate() {
            if lists[..index].iter().any(|other| other.name == list.name) {
                problems.push(format!(
                    "{BASELINE_PATH} lists the `{}` group twice",
                    list.name
                ));
                continue;
            }
            if let Some(why) = rewritten(&list.name) {
                problems.push(format!(
                    "{BASELINE_PATH} lists a group `{}`, a name the set may not use: \
                     {why}",
                    list.name
                ));
            }
            let listed: Vec<String> = list.operations.iter().map(|o| o.name.clone()).collect();
            for operation in &list.operations {
                if let Some(why) = rewritten(&operation.name) {
                    problems.push(format!(
                        "the `{}` list names `{}`, a name the set may not use: {why}",
                        list.name, operation.name
                    ));
                }
            }
            for name in twice(&listed) {
                problems.push(format!(
                    "the `{}` list in {BASELINE_PATH} holds `{name}` twice",
                    list.name
                ));
            }
            if !targets.contains(&list.name) {
                problems.push(format!(
                    "{BASELINE_PATH} lists a `{}` group, and {BENCH_MANIFEST} \
                     declares no `[[bench]]` of that name",
                    list.name
                ));
            }
        }
        for (group, file, names) in &defined {
            let Some(list) = lists.iter().find(|list| &list.name == group) else {
                problems.push(format!(
                    "{BASELINE_PATH} has no list for the `{group}` group `{file}` \
                     benchmarks"
                ));
                continue;
            };
            let listed: Vec<String> = list.operations.iter().map(|o| o.name.clone()).collect();
            let mut parted = false;
            for name in once(names) {
                if !listed.contains(name) {
                    parted = true;
                    problems.push(format!(
                        "`{file}` benchmarks `{name}`, which the `{group}` list in \
                         {BASELINE_PATH} does not hold"
                    ));
                }
            }
            for name in once(&listed) {
                if !names.contains(name) {
                    parted = true;
                    problems.push(format!(
                        "the `{group}` list in {BASELINE_PATH} holds `{name}`, which \
                         `{file}` does not benchmark"
                    ));
                }
            }
            if !parted && once(names) != once(&listed) {
                problems.push(format!(
                    "`{file}` and the `{group}` list in {BASELINE_PATH} name the same \
                     operations in different orders; a list is in the order its \
                     file declares them"
                ));
            }
        }
    }

    // One matrix entry a group, counting what the group's file defines.
    match std::fs::read_to_string(root.join(WORKFLOW_PATH)) {
        Err(error) => problems.push(format!("cannot read {WORKFLOW_PATH}: {error}")),
        Ok(workflow) => {
            if !workflow.contains(GUARD) {
                problems.push(format!(
                    "{WORKFLOW_PATH} has no `{GUARD}` guard; a job whose bench \
                     compiled and measured nothing exits 0"
                ));
            }
            let entries = guard_counts(&workflow);
            for (index, (group, count)) in entries.iter().enumerate() {
                if entries[..index].iter().any(|(other, _)| other == group) {
                    problems.push(format!(
                        "{WORKFLOW_PATH}'s matrix names `{group}` twice; one job a \
                         group, counting all of it"
                    ));
                    continue;
                }
                if !targets.contains(group) {
                    problems.push(format!(
                        "{WORKFLOW_PATH}'s matrix has an entry for `{group}`, which \
                         {BENCH_MANIFEST} declares no `[[bench]]` for"
                    ));
                    continue;
                }
                let Some(count) = count else {
                    problems.push(format!(
                        "{WORKFLOW_PATH}'s matrix entry for `{group}` has no `count`; \
                         its job's guard would compare against nothing"
                    ));
                    continue;
                };
                if let Some((_, file, names)) = defined.iter().find(|(g, _, _)| g == group) {
                    if *count as usize != names.len() {
                        problems.push(format!(
                            "{WORKFLOW_PATH} requires {count} criterion lines of the \
                             `{group}` job and `{file}` defines {} operations; a guard \
                             that disagrees with its file passes on the smaller number \
                             or fails every run",
                            names.len()
                        ));
                    }
                }
            }
            for target in &targets {
                if !entries.iter().any(|(group, _)| group == target) {
                    problems.push(format!(
                        "{WORKFLOW_PATH}'s matrix has no entry for the `{target}` \
                         group; no job measures it"
                    ));
                }
            }
        }
    }

    // Each machine's figures: its swing the worst of them, and every listed
    // operation either measured or named unmeasured, never both or neither.
    if let Some(machines) = &machines {
        let ids: Option<Vec<String>> = lists.as_ref().map(|lists| {
            lists
                .iter()
                .flat_map(|list| list.operations.iter().map(|o| list.id(&o.name)))
                .collect()
        });
        for machine in machines {
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
            let Some(ids) = &ids else { continue };
            for (index, operation) in machine.operations.iter().enumerate() {
                let id = &operation.id;
                if machine.operations[..index]
                    .iter()
                    .any(|other| &other.id == id)
                {
                    problems.push(format!(
                        "`{}` records `{id}` twice; the comparison reads the first \
                         and never the second",
                        machine.name
                    ));
                    continue;
                }
                if !ids.contains(id) {
                    problems.push(format!(
                        "`{}` records `{id}`, which no list in {BASELINE_PATH} holds",
                        machine.name
                    ));
                }
                if machine.unmeasured.contains(id) {
                    problems.push(format!(
                        "`{}` records `{id}` and names it unmeasured; an operation \
                         has a figure or a hole, never both",
                        machine.name
                    ));
                }
            }
            for (index, id) in machine.unmeasured.iter().enumerate() {
                if machine.unmeasured[..index].contains(id) {
                    problems.push(format!("`{}` names `{id}` unmeasured twice", machine.name));
                } else if !ids.contains(id) {
                    problems.push(format!(
                        "`{}` names `{id}` unmeasured, which no list in \
                         {BASELINE_PATH} holds",
                        machine.name
                    ));
                }
            }
            for id in ids {
                let measured = machine.operations.iter().any(|op| &op.id == id);
                if !measured && !machine.unmeasured.contains(id) {
                    problems.push(format!(
                        "`{}` has no figure for `{id}` and does not name it \
                         unmeasured; a hole is named, never silent, or the ratchet \
                         lets the operation regress without limit",
                        machine.name
                    ));
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
        if machine.unmeasured.contains(id) {
            println!("  unmeasured, as the baseline names it: {id}");
        } else {
            println!("  new, with no recorded figure: {id}");
        }
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

    /// The one that catches drift in the tree rather than in a fixture: the
    /// committed baseline, the group files and their `[[bench]]` entries, and
    /// the counts `bench.yml`'s matrix guards, held to each other as they
    /// stand.
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

    /// rustfmt moves a call's arguments onto lines of their own when they do
    /// not fit on one, and the name with them.
    #[test]
    fn a_name_rustfmt_moves_onto_its_own_line_is_read() {
        let source = "    group.bench_function(\n        \
                      \"render a full-page axial shading at 150 dpi\",\n        \
                      |b| {},\n    );\n";
        assert_eq!(
            literals_passed_to(source, "bench_function"),
            vec!["render a full-page axial shading at 150 dpi".to_string()]
        );
    }

    #[test]
    fn the_guard_counts_are_read_out_of_the_workflows_matrix() {
        assert_eq!(
            guard_counts(MATRIX),
            vec![
                ("open".to_string(), Some(1)),
                ("render".to_string(), Some(2))
            ]
        );
        // An entry with no count is read as one, so `check` can say so; a
        // commented entry is no entry.
        assert_eq!(
            guard_counts("          - group: open\n          # - group: text\n"),
            vec![("open".to_string(), None)]
        );
        assert_eq!(guard_counts("cargo bench -p tinker-pdf\n"), vec![]);
    }

    #[test]
    fn the_bench_targets_are_read_out_of_the_manifest() {
        assert_eq!(
            bench_targets(&format!(
                "{MANIFEST}\n[[test]]\nname = \"not a bench\"\n\n\
                 [dev-dependencies]\nname = \"nor this\"\n"
            )),
            vec!["open".to_string(), "render".to_string()]
        );
    }

    /// A stale name: a figure for an operation no list holds can never be
    /// compared, so it is refused rather than kept as a decoration.
    #[test]
    fn a_baseline_naming_an_operation_nobody_benchmarks_is_refused() {
        let mut tree = Tree::agreeing();
        tree.baseline = tree.baseline.replace(
            "\"id\": \"open/open a 3-page document\"",
            "\"id\": \"open/open a document nobody opens\"",
        );
        let problems = tree.problems("stale");
        assert!(
            problems
                .iter()
                .any(|p| p.contains("records `open/open a document nobody opens`, which no list")),
            "{problems:?}"
        );
    }

    /// The failure this row was opened on: the path named a file that was not
    /// there, so the step that promises to report and not fail exited 1 on
    /// every run of the weekly job.
    #[test]
    fn a_baseline_that_is_not_there_is_refused_by_name() {
        let root = Tree::agreeing().stage("absent");
        std::fs::remove_file(root.join(BASELINE_PATH)).expect("the staged baseline");
        let problems = check(&root).expect_err("there is no baseline");
        std::fs::remove_dir_all(&root).ok();
        assert!(
            problems.iter().any(|p| p.contains(BASELINE_PATH)),
            "{problems:?}"
        );
    }

    /// Criterion's own numbers, read out of its own file rather than timed
    /// again here, keyed by the directory it wrote them under: a group's
    /// operation under `<group>/<name>`.
    #[test]
    fn the_mean_comes_out_of_criterions_estimates() {
        let dir =
            std::env::temp_dir().join(format!("tinker-bench-check-test-{}", std::process::id()));
        let estimates = r#"{"mean":{"confidence_interval":{"confidence_level":0.95,
               "lower_bound":11892.0,"upper_bound":11963.0},
               "point_estimate":11921.5,"standard_error":18.0}}"#;
        for id in ["open a 3-page document", "open/open a 3-page document"] {
            let bench = dir.join(id).join("new");
            std::fs::create_dir_all(&bench).expect("a directory");
            std::fs::write(bench.join("estimates.json"), estimates).expect("a file");
        }
        let found = measured(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(found.len(), 2, "{found:?}");
        assert!((found["open a 3-page document"] - 11921.5).abs() < 0.01);
        assert!((found["open/open a 3-page document"] - 11921.5).abs() < 0.01);
    }

    // ---- one file a group ----------------------------------------------------

    /// Everything [`check`] reads, staged in a directory of its own, so each
    /// disagreement can be shown alone.
    struct Tree {
        manifest: String,
        files: Vec<(String, String)>,
        baseline: String,
        workflow: String,
    }

    const MANIFEST: &str = r#"[package]
name = "tinker-pdf"

[[bench]]
name = "open"
harness = false

[[bench]]
name = "render"
harness = false
"#;

    const OPEN_RS: &str = r#"mod support;

fn open(c: &mut Criterion) {
    let mut group = Group::new(c, "open");
    group.operation("open a 3-page document", |op| {});
    group.finish();
}
"#;

    /// Its second name split over lines, the way rustfmt writes a call whose
    /// arguments do not fit on one.
    const RENDER_RS: &str = r#"mod support;

fn render(c: &mut Criterion) {
    let mut group = Group::new(c, "render");
    group.operation("render text at 150 dpi", |op| {});
    group.operation(
        "render a full-page axial shading at 150 dpi",
        |op| {},
    );
    group.finish();
}
"#;

    const LISTS: &str = r#"{
  "note": ["Two groups whose files, lists and counts agree."],
  "groups": [
    {
      "group": "open",
      "operations": [
        { "name": "open a 3-page document", "unit": "bytes",
          "per_iteration": 10985, "calls": 1 }
      ]
    },
    {
      "group": "render",
      "operations": [
        { "name": "render text at 150 dpi", "unit": "pixels",
          "per_iteration": 2176200, "calls": 1 },
        { "name": "render a full-page axial shading at 150 dpi", "unit": "pixels",
          "per_iteration": 2176200, "calls": 1 }
      ]
    }
  ],
  "machines": [
    {
      "machine": "example",
      "runs": 5,
      "swing_percent": 6.0,
      "band_percent": 12.0,
      "recorded": "5 September 2026",
      "unmeasured": [],
      "operations": [
        { "id": "open/open a 3-page document", "nanos": 11921.0,
          "swing_percent": 3.0, "band_percent": 10.0 },
        { "id": "render/render text at 150 dpi", "nanos": 5341153.3,
          "swing_percent": 6.0, "band_percent": 11.0 },
        { "id": "render/render a full-page axial shading at 150 dpi",
          "nanos": 123662267.4, "swing_percent": 6.0, "band_percent": 11.0 }
      ]
    }
  ]
}"#;

    const MATRIX: &str = r#"jobs:
  bench:
    strategy:
      fail-fast: false
      matrix:
        include:
          - group: open
            count: 1
          - group: render
            count: 2
    steps:
      - run: |
          cargo bench -p tinker-pdf --bench ${{ matrix.group }} 2>&1 | tee -a bench.log
          test "$(grep -Ec 'time: +\[' bench.log)" -eq ${{ matrix.count }}
"#;

    impl Tree {
        /// Two groups that agree: `open` with one operation and `render`
        /// with two, beside the support module, which is no group.
        fn agreeing() -> Tree {
            Tree {
                manifest: MANIFEST.to_string(),
                files: vec![
                    ("open.rs".to_string(), OPEN_RS.to_string()),
                    ("render.rs".to_string(), RENDER_RS.to_string()),
                    (
                        "support/mod.rs".to_string(),
                        "//! What the group files share.\n".to_string(),
                    ),
                ],
                baseline: LISTS.to_string(),
                workflow: MATRIX.to_string(),
            }
        }

        /// A staged file's source, to change in place.
        fn file(&mut self, name: &str) -> &mut String {
            &mut self
                .files
                .iter_mut()
                .find(|(staged, _)| staged == name)
                .expect("a staged file")
                .1
        }

        /// Every occurrence of `from` in every file, written as `to`.
        fn rename(&mut self, from: &str, to: &str) {
            for (_, source) in &mut self.files {
                *source = source.replace(from, to);
            }
            self.baseline = self.baseline.replace(from, to);
        }

        /// This tree written out under a root of its own, named for `tag` and
        /// this process, so two checkouts testing at once never share one.
        fn stage(&self, tag: &str) -> PathBuf {
            let root =
                std::env::temp_dir().join(format!("tinker-baseline-{tag}-{}", std::process::id()));
            std::fs::remove_dir_all(&root).ok();
            let benches = root.join(BENCH_DIR);
            std::fs::create_dir_all(benches.join("support")).expect("a directory");
            std::fs::write(root.join(BENCH_MANIFEST), &self.manifest).expect("a file");
            for (name, source) in &self.files {
                std::fs::write(benches.join(name), source).expect("a file");
            }
            std::fs::write(root.join(BASELINE_PATH), &self.baseline).expect("a file");
            let workflow = root.join(WORKFLOW_PATH);
            std::fs::create_dir_all(workflow.parent().expect("a directory")).expect("a directory");
            std::fs::write(workflow, &self.workflow).expect("a file");
            root
        }

        /// What `check` says of this tree, staged under `tag`.
        fn problems(&self, tag: &str) -> Vec<String> {
            let root = self.stage(tag);
            let verdict = check(&root);
            std::fs::remove_dir_all(&root).ok();
            verdict.err().unwrap_or_default()
        }
    }

    /// One shape of a class: what it changes in an agreeing tree, and the
    /// words the refusal must say.
    type Shape = (&'static str, fn(&mut Tree), &'static str);

    /// Holds `check` to refusing every shape, each with the words it names.
    fn refuses(test: &str, shapes: &[Shape]) {
        let mut wrong = Vec::new();
        for (index, (shape, change, words)) in shapes.iter().enumerate() {
            let mut tree = Tree::agreeing();
            change(&mut tree);
            let problems = tree.problems(&format!("{test}-{index}"));
            if !problems.iter().any(|problem| problem.contains(words)) {
                wrong.push(format!("{shape}: no problem says `{words}`: {problems:?}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// The tree every refusal below starts from passes, so each refusal is
    /// its own change's and nothing else's.
    #[test]
    fn group_files_lists_and_counts_that_agree_pass() {
        assert_eq!(Tree::agreeing().problems("agrees"), Vec::<String>::new());
    }

    /// Count mode runs what `Group::operation` declares and nothing else, so
    /// its table is the names each group file hands `operation`. In the tree
    /// that table is every group's list, name for name and in order, but the
    /// frozen group's, whose file declares nothing through it; and a group
    /// file that reaches criterion's own `bench_function` or
    /// `benchmark_group` is refused, by the call.
    #[test]
    fn every_operation_runs_in_count_mode() {
        let root = repo_root();
        let text = std::fs::read_to_string(root.join(BASELINE_PATH)).expect("the committed file");
        let lists = groups(&text, BASELINE_PATH).expect("it parses");
        let mut counted = 0;
        for list in &lists {
            let file = format!("{BENCH_DIR}/{}.rs", list.name);
            let source = std::fs::read_to_string(root.join(&file)).expect("the group's file");
            let declared = literals_passed_to(&source, "operation");
            if FROZEN.contains(&list.name.as_str()) {
                assert!(
                    declared.is_empty(),
                    "{file} is frozen and declares nothing through `operation`: {declared:?}"
                );
                continue;
            }
            let listed: Vec<&str> = list.operations.iter().map(|op| op.name.as_str()).collect();
            assert_eq!(
                declared, listed,
                "{file}: count mode's operations and its list"
            );
            assert_eq!(
                literals_after(&source, GROUP_OPENED),
                std::slice::from_ref(&list.name)
            );
            assert!(
                !source.contains(".bench_function(") && !source.contains(".benchmark_group("),
                "{file} reaches criterion past count mode"
            );
            counted += declared.len();
        }
        assert!(counted > 0, "count mode runs no operation");

        refuses(
            "count-mode",
            &[
                (
                    "an operation benchmarked through criterion's own call",
                    |tree| {
                        let source = tree.file("render.rs");
                        *source = source.replace(
                            "group.operation(\"render text at 150 dpi\", |op| {});",
                            "group.bench_function(\"render text at 150 dpi\", |b| {});",
                        );
                    },
                    "calls criterion's `bench_function` itself",
                ),
                (
                    "a group opened through criterion's own call",
                    |tree| {
                        let source = tree.file("open.rs");
                        *source = source
                            .replace("Group::new(c, \"open\")", "c.benchmark_group(\"open\")");
                    },
                    "calls criterion's `benchmark_group` itself",
                ),
            ],
        );
    }

    /// A group file and its list in `baseline.json` carry one list of names,
    /// and a `[[bench]]` declares each file: every way the three can part.
    #[test]
    fn a_group_file_and_its_list_that_disagree_are_refused() {
        refuses(
            "disagree",
            &[
                (
                    "the file benchmarks a name its list does not hold",
                    |tree| {
                        let source = tree.file("render.rs");
                        *source = source.replace(
                            "    group.finish();",
                            "    group.operation(\"fill 300 anti-aliased paths at 150 dpi\", \
                             |op| {});\n    group.finish();",
                        );
                        tree.workflow = tree.workflow.replace("count: 2", "count: 3");
                    },
                    "benchmarks `fill 300 anti-aliased paths at 150 dpi`, which the `render` list",
                ),
                (
                    "the list holds a name its file does not benchmark",
                    |tree| {
                        let source = tree.file("render.rs");
                        *source = source.replace(
                            "    group.operation(\"render text at 150 dpi\", |op| {});\n",
                            "",
                        );
                        tree.workflow = tree.workflow.replace("count: 2", "count: 1");
                    },
                    "holds `render text at 150 dpi`, which `crates/tinker-pdf/benches/render.rs` \
                     does not benchmark",
                ),
                (
                    "the two name the same operations in another order",
                    |tree| {
                        let source = tree.file("render.rs");
                        *source = source
                            .replace("\"render text at 150 dpi\"", "\"FIRST\"")
                            .replace(
                                "\"render a full-page axial shading at 150 dpi\"",
                                "\"render text at 150 dpi\"",
                            )
                            .replace(
                                "\"FIRST\"",
                                "\"render a full-page axial shading at 150 dpi\"",
                            );
                    },
                    "in different orders",
                ),
                (
                    "the file benchmarks one name twice",
                    |tree| {
                        let source = tree.file("open.rs");
                        *source = source.replace(
                            "    group.finish();",
                            "    group.operation(\"open a 3-page document\", |op| {});\n    \
                             group.finish();",
                        );
                        tree.workflow = tree.workflow.replace("count: 1", "count: 2");
                    },
                    "benchmarks `open a 3-page document` twice",
                ),
                (
                    "a group file that is no `[[bench]]`",
                    |tree| {
                        tree.files.push((
                            "text.rs".to_string(),
                            "let mut group = Group::new(c, \"text\");\n\
                             group.operation(\"extract a page of text\", |op| {});\n"
                                .to_string(),
                        ));
                    },
                    "`crates/tinker-pdf/benches/text.rs` is no `[[bench]]`",
                ),
                (
                    "a `[[bench]]` whose group has no list",
                    |tree| {
                        tree.manifest
                            .push_str("\n[[bench]]\nname = \"text\"\nharness = false\n");
                        tree.files.push((
                            "text.rs".to_string(),
                            "let mut group = Group::new(c, \"text\");\n\
                             group.operation(\"extract a page of text\", |op| {});\n"
                                .to_string(),
                        ));
                        tree.workflow = tree.workflow.replace(
                            "    steps:",
                            "          - group: text\n            count: 1\n    steps:",
                        );
                    },
                    "no list for the `text` group",
                ),
                (
                    "a `[[bench]]` with no file",
                    |tree| {
                        tree.manifest
                            .push_str("\n[[bench]]\nname = \"text\"\nharness = false\n");
                    },
                    "cannot read crates/tinker-pdf/benches/text.rs",
                ),
                (
                    "a list for a group no `[[bench]]` declares",
                    |tree| {
                        tree.baseline = tree.baseline.replace(
                            "    }\n  ],\n  \"machines\": [",
                            "    },\n    { \"group\": \"text\", \"operations\": [\n      \
                             { \"name\": \"extract a page of text\", \"unit\": \"pages\", \
                             \"per_iteration\": 1, \"calls\": 1 } ] }\n  ],\n  \"machines\": [",
                        );
                        tree.baseline = tree.baseline.replace(
                            "\"unmeasured\": []",
                            "\"unmeasured\": [\"text/extract a page of text\"]",
                        );
                    },
                    "lists a `text` group",
                ),
                (
                    "a group file opening a group of another name",
                    |tree| {
                        let source = tree.file("open.rs");
                        *source =
                            source.replace("Group::new(c, \"open\")", "Group::new(c, \"opening\")");
                    },
                    "opens `opening`",
                ),
                (
                    "a group file opening no group",
                    |tree| {
                        let source = tree.file("open.rs");
                        *source = source.replace(
                            "let mut group = Group::new(c, \"open\");",
                            "let mut group = c;",
                        );
                    },
                    "opens no group",
                ),
            ],
        );
    }

    /// The guard is the one count in `bench.yml`, and a count in a shell
    /// script is exactly the kind that drifts: it said six for a day after
    /// `engine.rs` had seven. One job a group now, so one count a group, each
    /// held to its file — short, and long, which fails every run instead —
    /// and refused on that count alone.
    #[test]
    fn a_group_guard_counting_short_is_refused() {
        let mut wrong = Vec::new();
        for (index, (shape, count, words)) in [
            (
                "the render job counts one of its two",
                "count: 1",
                "requires 1 criterion lines of the `render` job and \
                 `crates/tinker-pdf/benches/render.rs` defines 2 operations",
            ),
            (
                "the render job counts three of its two",
                "count: 3",
                "requires 3 criterion lines of the `render` job and \
                 `crates/tinker-pdf/benches/render.rs` defines 2 operations",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let mut tree = Tree::agreeing();
            tree.workflow = tree.workflow.replace("count: 2", count);
            let problems = tree.problems(&format!("guard-{index}"));
            if problems.len() != 1 || !problems[0].contains(words) {
                wrong.push(format!(
                    "{shape}: not refused for `{words}` alone: {problems:?}"
                ));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// The matrix's other ways of not following the groups: a group no job
    /// measures, a job for no group, a group measured twice, an entry with no
    /// count, and a guard that reads no count at all.
    #[test]
    fn a_matrix_that_does_not_follow_the_groups_is_refused() {
        refuses(
            "matrix",
            &[
                (
                    "a group no entry names",
                    |tree| {
                        tree.workflow = tree
                            .workflow
                            .replace("          - group: render\n            count: 2\n", "");
                    },
                    "no entry for the `render` group",
                ),
                (
                    "an entry for a group no `[[bench]]` declares",
                    |tree| {
                        tree.workflow = tree.workflow.replace(
                            "    steps:",
                            "          - group: text\n            count: 1\n    steps:",
                        );
                    },
                    "an entry for `text`",
                ),
                (
                    "a group named by two entries",
                    |tree| {
                        tree.workflow = tree.workflow.replace(
                            "    steps:",
                            "          - group: open\n            count: 1\n    steps:",
                        );
                    },
                    "names `open` twice",
                ),
                (
                    "an entry with no count",
                    |tree| {
                        tree.workflow = tree.workflow.replace(
                            "          - group: open\n            count: 1\n",
                            "          - group: open\n",
                        );
                    },
                    "entry for `open` has no `count`",
                ),
                (
                    "a guard that compares against a number of its own",
                    |tree| {
                        tree.workflow = tree.workflow.replace("-eq ${{ matrix.count }}", "-eq 3");
                    },
                    "no `-eq ${{ matrix.count }}` guard",
                ),
            ],
        );
    }

    /// A hole is named, never silent: an operation a list holds has a figure
    /// in each machine entry, or that entry names it unmeasured. Before the
    /// lists, a baseline short of an operation was refused outright, so an
    /// operation could not be added before the set was measured.
    #[test]
    fn an_operation_neither_measured_nor_named_unmeasured_is_refused() {
        let mut tree = Tree::agreeing();
        tree.baseline = tree.baseline.replace(
            "        { \"id\": \"render/render text at 150 dpi\", \"nanos\": 5341153.3,\n          \
             \"swing_percent\": 6.0, \"band_percent\": 11.0 },\n",
            "",
        );
        let problems = tree.problems("neither");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains(
                "`example` has no figure for `render/render text at 150 dpi` and does not name it \
                 unmeasured"
            ),
            "{problems:?}"
        );

        // Its neighbour, which must pass: the same hole, named.
        tree.baseline = tree.baseline.replace(
            "\"unmeasured\": []",
            "\"unmeasured\": [\"render/render text at 150 dpi\"]",
        );
        assert_eq!(tree.problems("named"), Vec::<String>::new());
    }

    /// The other ways a machine entry can name an operation wrongly.
    #[test]
    fn a_machine_entry_naming_an_operation_wrongly_is_refused() {
        refuses(
            "machine",
            &[
                (
                    "an operation both measured and named unmeasured",
                    |tree| {
                        tree.baseline = tree.baseline.replace(
                            "\"unmeasured\": []",
                            "\"unmeasured\": [\"open/open a 3-page document\"]",
                        );
                    },
                    "records `open/open a 3-page document` and names it unmeasured",
                ),
                (
                    "an operation named unmeasured that no list holds",
                    |tree| {
                        tree.baseline = tree
                            .baseline
                            .replace("\"unmeasured\": []", "\"unmeasured\": [\"render/gone\"]");
                    },
                    "names `render/gone` unmeasured, which no list",
                ),
                (
                    "an operation recorded twice",
                    |tree| {
                        tree.baseline = tree.baseline.replace(
                            "\"operations\": [\n        { \"id\": \"open/open a 3-page document\"",
                            "\"operations\": [\n        { \"id\": \"open/open a 3-page document\", \
                             \"nanos\": 11921.0, \"swing_percent\": 3.0, \"band_percent\": 10.0 },\n        \
                             { \"id\": \"open/open a 3-page document\"",
                        );
                    },
                    "records `open/open a 3-page document` twice",
                ),
                (
                    "the unmeasured ids after the operations",
                    |tree| {
                        tree.baseline = tree.baseline.replace("      \"unmeasured\": [],\n", "");
                        tree.baseline = tree.baseline.replace(
                            "\"band_percent\": 11.0 }\n      ]\n    }\n  ]",
                            "\"band_percent\": 11.0 }\n      ],\n      \"unmeasured\": []\n    }\n  ]",
                        );
                    },
                    "after its `operations`",
                ),
            ],
        );
    }

    /// Each list entry carries a unit of the five, and a `per_iteration` and
    /// `calls` of one or more; each list holds an operation; the lists come
    /// ahead of the machines.
    #[test]
    fn a_list_outside_the_format_is_refused() {
        refuses(
            "format",
            &[
                (
                    "a unit outside the five",
                    |tree| {
                        tree.baseline = tree
                            .baseline
                            .replace("\"unit\": \"bytes\"", "\"unit\": \"seconds\"")
                    },
                    "no unit of the five",
                ),
                (
                    "no unit",
                    |tree| {
                        tree.baseline = tree
                            .baseline
                            .replace("\"unit\": \"bytes\"", "\"kind\": \"bytes\"")
                    },
                    "no unit of the five",
                ),
                (
                    "a per_iteration of none",
                    |tree| {
                        tree.baseline = tree
                            .baseline
                            .replace("\"per_iteration\": 10985", "\"per_iteration\": 0")
                    },
                    "no `per_iteration` of one or more",
                ),
                (
                    "no per_iteration",
                    |tree| {
                        tree.baseline = tree
                            .baseline
                            .replace("\"per_iteration\": 10985", "\"covers\": 10985")
                    },
                    "no `per_iteration` of one or more",
                ),
                (
                    "a calls of none",
                    |tree| {
                        tree.baseline = tree.baseline.replacen("\"calls\": 1", "\"calls\": 0", 1)
                    },
                    "no `calls` of one or more",
                ),
                (
                    "a list that holds no operation",
                    |tree| {
                        tree.baseline = tree.baseline.replace(
                            "      \"operations\": [\n        { \"name\": \"open a 3-page document\", \
                             \"unit\": \"bytes\",\n          \"per_iteration\": 10985, \"calls\": 1 }\n      ]",
                            "      \"operations\": []",
                        );
                    },
                    "the `open` list holds no operation",
                ),
                (
                    "no lists at all",
                    |tree| {
                        let start = tree.baseline.find("  \"groups\"").expect("the lists");
                        let end = tree.baseline.find("  \"machines\"").expect("the machines");
                        tree.baseline.replace_range(start..end, "");
                    },
                    "lists no group ahead of its machines",
                ),
            ],
        );
    }

    /// Criterion 0.5.1 keys an operation by a directory it makes of the name:
    /// ten characters become `_` and the name is cut at 64 bytes, so a name
    /// either rule touches is recorded under a key the baseline does not
    /// hold, reported new and never held to its band. And a name keeps to
    /// printable ASCII, the set's own rule. Each shape is written into the
    /// file, its list and its figure alike, so nothing else disagrees; `"`
    /// and `\` are written escaped, as a literal and a JSON string carry
    /// them, and the readers stop at the escape.
    #[test]
    fn an_operation_name_criterion_would_rewrite_is_refused() {
        let long = format!("open {}", "a".repeat(60));
        let longest = format!("open {}", "a".repeat(59));
        assert_eq!((long.len(), longest.len()), (65, 64));
        // (shape, the name as written, the words its refusal says, or none
        // for a neighbour that must pass)
        let mut shapes: Vec<(String, String, Option<&str>)> =
            ["?", "\\\"", "/", "\\\\", "*", "<", ">", ":", "|", "^"]
                .iter()
                .map(|written| {
                    (
                        format!("a name holding {written}"),
                        format!("open a{written}document"),
                        Some("under another key"),
                    )
                })
                .collect();
        shapes.push((
            "a name of 65 bytes".to_string(),
            long,
            Some("under another key"),
        ));
        shapes.push((
            "a name outside ASCII".to_string(),
            "open a 3\u{2011}page document".to_string(),
            Some("outside printable ASCII"),
        ));
        shapes.push(("a name of 64 bytes".to_string(), longest, None));
        shapes.push((
            "a name spelt the way the rules ask".to_string(),
            "PDF-A validation of 100 pages".to_string(),
            None,
        ));

        let mut wrong = Vec::new();
        for (index, (shape, written, words)) in shapes.iter().enumerate() {
            let mut tree = Tree::agreeing();
            tree.rename("open a 3-page document", written);
            let problems = tree.problems(&format!("rewrite-{index}"));
            match words {
                Some(words) if !problems.iter().any(|p| p.contains(words)) => {
                    wrong.push(format!("{shape}: no problem says `{words}`: {problems:?}"));
                }
                None if !problems.is_empty() => {
                    wrong.push(format!("{shape}: refused: {problems:?}"));
                }
                _ => {}
            }
        }

        // A group's name is a part of the same path.
        let mut tree = Tree::agreeing();
        let group = "o".repeat(65);
        tree.manifest = tree
            .manifest
            .replace("name = \"open\"", &format!("name = \"{group}\""));
        tree.files[0].0 = format!("{group}.rs");
        tree.files[0].1 = tree.files[0].1.replace("\"open\"", &format!("\"{group}\""));
        tree.baseline = tree
            .baseline
            .replace("\"group\": \"open\"", &format!("\"group\": \"{group}\""))
            .replace("\"open/open", &format!("\"{group}/open"));
        tree.workflow = tree
            .workflow
            .replace("group: open", &format!("group: {group}"));
        let problems = tree.problems("rewrite-group");
        if !problems
            .iter()
            .any(|p| p.contains(&format!("a group `{group}`")) && p.contains("under another key"))
        {
            wrong.push(format!(
                "a group name of 65 bytes: not refused: {problems:?}"
            ));
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// The seven moved into their groups' files keep the figures fifteen
    /// dispatches recorded for them on 19 September 2026: re-keyed under
    /// their group ids, never re-measured or re-banded (MS-04.2).
    #[test]
    fn the_seven_keep_their_figures_under_their_group_ids() {
        // (id, the id it had, nanos, swing_percent, band_percent), as
        // `a8425fac` recorded them.
        const SEVEN: [(&str, &str, f64, f64, f64); 7] = [
            (
                "open/open a 3-page document",
                "open a 3-page document",
                14527.1,
                33.67,
                55.0,
            ),
            (
                "text/extract a page of text",
                "extract a page of text",
                728484.6,
                22.61,
                35.0,
            ),
            (
                "render/render text at 150 dpi",
                "render text at 150 dpi",
                5341153.3,
                88.36,
                135.0,
            ),
            (
                "render/render a full-page axial shading at 150 dpi",
                "shading/render a full-page axial shading at 150 dpi",
                123662267.4,
                38.80,
                60.0,
            ),
            (
                "render/fill 300 anti-aliased paths at 150 dpi",
                "fill 300 anti-aliased paths at 150 dpi",
                31738504.1,
                42.39,
                65.0,
            ),
            (
                "write/rewrite a document",
                "rewrite a document",
                51980.8,
                27.89,
                45.0,
            ),
            (
                "layout/paginate a book at 432x648 pt",
                "paginate a book at 432x648 pt",
                12365318.6,
                30.22,
                50.0,
            ),
        ];
        let text =
            std::fs::read_to_string(repo_root().join(BASELINE_PATH)).expect("the committed file");
        let machines = parse(&text, BASELINE_PATH).expect("it parses");
        let machine = machines
            .iter()
            .find(|machine| machine.name == "ubuntu-latest")
            .expect("the named machine");
        assert_eq!(machine.runs, 15);
        assert!((machine.swing_percent - 88.36).abs() < 1e-9);
        assert!((machine.band_percent - 135.0).abs() < 1e-9);
        assert_eq!(
            machine.recorded,
            "19 September 2026, fifteen workflow_dispatch runs of bench.yml on develop at 4aff8cc"
        );
        for dispatch in [
            "35462488069",
            "35462702958",
            "35462705081",
            "35462706950",
            "35462708751",
            "35462921377",
            "35462922858",
            "35462924746",
            "35462926404",
            "35463256473",
            "35463258487",
            "35463260359",
            "35463262176",
            "35463264168",
            "35463266192",
        ] {
            assert!(text.contains(dispatch), "dispatch {dispatch} is gone");
        }
        let mut wrong = Vec::new();
        for (id, was, nanos, swing, band) in SEVEN {
            match machine
                .operations
                .iter()
                .find(|operation| operation.id == id)
            {
                None => wrong.push(format!("`{id}`, once `{was}`, has no figure")),
                Some(operation) => {
                    if (operation.nanos - nanos).abs() > 1e-6
                        || (operation.swing_percent - swing).abs() > 1e-9
                        || (operation.band_percent - band).abs() > 1e-9
                    {
                        wrong.push(format!(
                            "`{id}` moved: {} ns, swing {} %, band {} % where `{was}` had \
                             {nanos} ns, {swing} %, {band} %",
                            operation.nanos, operation.swing_percent, operation.band_percent
                        ));
                    }
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
        assert_eq!(
            machine.operations.len(),
            7,
            "the seven, and no figure beside them"
        );
    }
}
