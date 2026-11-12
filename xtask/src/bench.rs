//! The speed ratchet: a committed baseline, and a comparison that can fail.
//!
//! # Why this exists rather than `cargo bench -- --baseline`
//!
//! Criterion compares against a saved baseline perfectly well and **never exits
//! non-zero when it loses**. The only failure path in 0.5.1 is a *missing*
//! baseline directory; a regression prints "Performance has regressed" and the
//! process exits 0, so a run that leaned on it would report rather than gate.
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
//! `[[bench]]`, and `cargo bench --bench <group>` measures one group (MS-04).
//! So the baseline lists each group's operations ahead of its machines, in the
//! order the
//! group's file declares them, each with its unit, what one iteration covers
//! and how many calls it makes ([`Listed`]). A machine entry names the
//! operations it has no figure for yet ([`Machine::unmeasured`]): before the
//! lists, an entry short of any operation was refused outright, so no
//! operation could join the set until the whole set was measured again, and a
//! hole the entry names is one nobody can miss. [`check`] holds the
//! `[[bench]]` entries, each file's names, its list and every machine entry to
//! each other.
//!
//! # Why the gates sit in the lists
//!
//! Tier P states every target against one revision, `61adcde`, the planning
//! baseline, and its Gate column says how a run on any machine checks it: a
//! factor floor, held, a target calibrated against an operation the planning
//! baseline times beside it, or a relation between two operations of one
//! build. Those belong to an operation and not to a machine, so each
//! operation's list entry carries its own [`Gate`], where the machine reader,
//! which cuts its entries at `"machine":` and `"id":`, never reaches it.
//! [`check`] refuses an operation with no gate, so none joins the set unheld
//! against the baseline, and [`against_planning`] holds a head's rounds to the
//! planning baseline's by them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where the committed baseline lives.
///
/// Hand-written, and key order is load-bearing. The calibration comes first,
/// then the lists, and the machines after them: [`calibration`] reads only
/// what lies before `"groups":`, [`parse`] reads machines only after
/// `"machines":` and [`groups`] reads lists only before it, so no list entry,
/// and no gate inside one, can fall into a machine's chunk. Within the machines, `"machine"` is deliberately the first
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
    /// The band `bench-check --paired` holds its median paired ratio to, in
    /// per cent, from the entry's `paired` object; `None` until the A/A
    /// dispatches have set one, and an operation with none is reported,
    /// never failed.
    pub paired_band: Option<f64>,
    /// Why the operation runs outside [`SIZE_WINDOW_MS`], in the owner's
    /// words, from the entry's `outside_window`; `None` for every
    /// operation sized inside it, which `--sizes` holds there.
    pub outside_window: Option<String>,
    /// What the planning baseline pair holds it to, from the entry's
    /// `gate`: tier P's Gate column. `None` where the entry has none, which
    /// [`check`] refuses in the tree and a side read for its `calls` alone
    /// never asks for.
    pub gate: Option<Gate>,
}

/// What an operation's time is held to against the planning baseline,
/// `61adcde`, in the paired run that builds it beside the head: tier P's
/// Gate column, transcribed into the operation's `gate`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Against {
    /// A factor floor: the head's per-call time over the planning
    /// baseline's, round by round, at most 1 ÷ the floor. Floors start at
    /// 1, a P row raises its operations' to the column's factors in the
    /// commit that closes it, and only the owner lowers one (H-78).
    Floor(f64),
    /// Held: a floor of 1 that no row raises.
    Held,
    /// Calibrated, for an operation the planning baseline lacks: its target
    /// in milliseconds on the planning machine, held as that fraction of the
    /// [`Calibration`] operation the planning baseline timed in the same
    /// round.
    Calibrated(f64),
}

/// How one operation's per-call time is held to another's of its group,
/// both timed by the head in the same round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bound {
    /// Its time at most this many times the other's.
    AtMost(f64),
    /// At least this many times as fast as the other: the other's time at
    /// least this many times its own.
    SpeedupAtLeast(f64),
}

impl Bound {
    /// The most its time may be over the other's.
    #[must_use]
    pub fn limit(self) -> f64 {
        match self {
            Bound::AtMost(times) => times,
            Bound::SpeedupAtLeast(times) => 1.0 / times,
        }
    }
}

/// A relation: the other operation of the group, and the bound.
#[derive(Clone, Debug, PartialEq)]
pub struct Relation {
    /// The other operation's name in the group.
    pub to: String,
    /// What the two per-call times are held to.
    pub bound: Bound,
}

/// One operation's gate: what holds it against the planning baseline, and
/// its relations to the other operations of its group.
#[derive(Clone, Debug, PartialEq)]
pub struct Gate {
    /// A floor, held, or a calibrated target.
    pub against: Against,
    /// Each relation, in the order the entry gives them.
    pub relations: Vec<Relation>,
}

/// The operation every calibrated target is a fraction of, named once
/// ahead of the groups: the planning baseline's figure for it is what a
/// target in milliseconds was stated against.
#[derive(Clone, Debug, PartialEq)]
pub struct Calibration {
    /// Its id, `<group>/<name>`.
    pub operation: String,
    /// Its time on the planning machine, in milliseconds.
    pub planning_ms: f64,
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
    /// What makes a pinned machine that machine, or `None` for a fleet such
    /// as `ubuntu-latest`, whose runs land on hosts no entry can name.
    pub pinned: Option<Pinned>,
}

/// One host, held still: what a pinned machine's entry says it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Pinned {
    /// The host's CPU, as `/proc/cpuinfo`'s first `model name` line gives it.
    pub cpu: String,
    /// The frequency and core isolation it is held at, as its entry states
    /// them.
    pub held: String,
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
    /// A listed operation's `gate` is there and outside the format.
    Gate {
        /// Which group's list.
        group: String,
        /// Which operation.
        name: String,
        /// What is wrong with it.
        why: String,
    },
    /// The calibration named ahead of the groups is outside the format.
    Calibration(String),
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
                 it has not measured, and a pinned machine's host, come first"
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
            BaselineError::Gate { group, name, why } => write!(
                f,
                "the `{group}` list gives `{name}` a gate outside the format: {why}"
            ),
            BaselineError::Calibration(why) => write!(
                f,
                "the calibration ahead of the groups is outside the format: {why}"
            ),
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
        // A pinned machine names its host and how it is held, both ahead of
        // its operations, and a fleet names neither: one without the other
        // is no pin.
        for key in ["cpu", "held"] {
            if body.contains(&format!("\"{key}\":")) {
                return Err(BaselineError::Misplaced {
                    machine: name.clone(),
                    key,
                });
            }
        }
        let pinned = match (
            string_after(head, "\"cpu\":").filter(|cpu| !cpu.is_empty()),
            string_after(head, "\"held\":").filter(|held| !held.is_empty()),
        ) {
            (None, None) if !head.contains("\"cpu\":") && !head.contains("\"held\":") => None,
            (Some(cpu), Some(held)) => Some(Pinned { cpu, held }),
            (cpu, _) => {
                return Err(BaselineError::Field {
                    machine: name.clone(),
                    key: if cpu.is_none() { "cpu" } else { "held" },
                })
            }
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
            pinned,
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
            let paired_band = match entry.split_once("\"paired\":") {
                Some((_, paired)) => Some(
                    number_after(paired, "band_percent")
                        .filter(|band| *band > 0.0)
                        .ok_or_else(|| refused("paired"))?,
                ),
                None => None,
            };
            let outside_window = string_after(entry, "\"outside_window\":")
                .filter(|reason| !reason.trim().is_empty());
            let per_call = (unit != Unit::None).then(|| per_iteration as f64 / calls as f64);
            let gate = match entry.find(GATE) {
                Some(at) => Some(
                    object_at(&entry[at + GATE.len()..])
                        .ok_or_else(|| "no object follows `gate`".to_string())
                        .and_then(|(object, _)| gate_in(object, per_call))
                        .map_err(|why| BaselineError::Gate {
                            group: group.clone(),
                            name: name.clone(),
                            why,
                        })?,
                ),
                None => None,
            };
            operations.push(Listed {
                name,
                unit,
                per_iteration,
                calls,
                paired_band,
                outside_window,
                gate,
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

/// The key an operation's gate follows in its list entry.
const GATE: &str = "\"gate\":";

/// The key the calibration follows, ahead of the groups.
const CALIBRATION: &str = "\"calibration\":";

/// The keys of which a gate names exactly one: what holds it against the
/// planning baseline.
const AGAINST: [&str; 4] = ["floor", "held", "calibrated_ms", "calibrated_per_second"];

/// One gate, read from the text inside its braces, for an operation one
/// call of which processes `per_call` of its unit (`None` for the unit
/// `none`): exactly one of `floor`, a number of 1 or more; `held`, `true`;
/// `calibrated_ms`, a target above 0 in milliseconds a call; and
/// `calibrated_per_second`, a target above 0 in its unit a second, read as
/// the milliseconds a call takes at that rate; and `relations`, an array of
/// objects each naming the other operation in `to` and one bound above 0,
/// `at_most` or `speedup_at_least`. Why it is outside the format, otherwise.
fn gate_in(object: &str, per_call: Option<f64>) -> Result<Gate, String> {
    // The relations' own keys never reach the gate's, wherever the array
    // stands among them.
    let (own, relations) = match object.find(RELATIONS) {
        Some(at) => {
            let (inside, after) = enclosed(&object[at + RELATIONS.len()..], '[', ']')
                .ok_or_else(|| "no array follows `relations`".to_string())?;
            (format!("{}{after}", &object[..at]), Some(inside))
        }
        None => (object.to_string(), None),
    };
    let own = own.as_str();
    let named: Vec<&str> = AGAINST
        .into_iter()
        .filter(|key| own.contains(&format!("\"{key}\":")))
        .collect();
    let against = match named.as_slice() {
        ["floor"] => match number_after(own, "floor") {
            Some(floor) if floor >= 1.0 => Against::Floor(floor),
            Some(floor) => {
                return Err(format!(
                    "a floor of {floor}, under 1, lets it lose time against the planning \
                     baseline, which only the owner grants (H-78)"
                ))
            }
            None => return Err("`floor` is no number".to_string()),
        },
        ["held"] => {
            let at = own.find("\"held\":").unwrap_or_default() + "\"held\":".len();
            if !own[at..].trim_start().starts_with("true") {
                return Err("`held` is not `true`".to_string());
            }
            Against::Held
        }
        ["calibrated_ms"] => match number_after(own, "calibrated_ms") {
            Some(ms) if ms > 0.0 => Against::Calibrated(ms),
            _ => return Err("`calibrated_ms` is no number above 0".to_string()),
        },
        ["calibrated_per_second"] => {
            let Some(per_call) = per_call else {
                return Err(
                    "a rate is a quantity a second, and the unit `none` counts none: \
                     give `calibrated_ms`"
                        .to_string(),
                );
            };
            match number_after(own, "calibrated_per_second") {
                Some(rate) if rate > 0.0 => Against::Calibrated(per_call / rate * 1_000.0),
                _ => return Err("`calibrated_per_second` is no number above 0".to_string()),
            }
        }
        [] => {
            return Err(format!(
                "it names none of {}, so nothing holds it against the planning baseline",
                AGAINST.map(|key| format!("`{key}`")).join(", ")
            ))
        }
        _ => {
            return Err(format!(
                "it names {}, and an operation is held one way",
                named
                    .iter()
                    .map(|key| format!("`{key}`"))
                    .collect::<Vec<_>>()
                    .join(" and ")
            ))
        }
    };
    let mut out = Vec::new();
    if let Some(mut rest) = relations {
        loop {
            rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
            if rest.is_empty() {
                break;
            }
            let (relation, after) =
                object_at(rest).ok_or_else(|| "a relation is no object".to_string())?;
            rest = after;
            let to = string_after(relation, "\"to\":")
                .filter(|to| !to.is_empty())
                .ok_or_else(|| "a relation names no operation in `to`".to_string())?;
            let bound = match (
                relation
                    .contains("\"at_most\":")
                    .then(|| number_after(relation, "at_most")),
                relation
                    .contains("\"speedup_at_least\":")
                    .then(|| number_after(relation, "speedup_at_least")),
            ) {
                (Some(Some(times)), None) if times > 0.0 => Bound::AtMost(times),
                (None, Some(Some(times))) if times > 0.0 => Bound::SpeedupAtLeast(times),
                (Some(_), Some(_)) => {
                    return Err(format!(
                        "the relation to `{to}` names both `at_most` and \
                         `speedup_at_least`"
                    ))
                }
                _ => {
                    return Err(format!(
                        "the relation to `{to}` names no bound above 0, `at_most` or \
                         `speedup_at_least`"
                    ))
                }
            };
            out.push(Relation { to, bound });
        }
    }
    Ok(Gate {
        against,
        relations: out,
    })
}

/// The key a gate's relations follow.
const RELATIONS: &str = "\"relations\":";

/// The text inside the braces of the object that opens `text`, whitespace
/// before it allowed, and what follows its closing brace; `None` when no
/// object opens there or it never closes.
fn object_at(text: &str) -> Option<(&str, &str)> {
    enclosed(text, '{', '}')
}

/// The text between `open`, which opens `text` once whitespace is passed,
/// and the `close` that matches it, and what follows that; `None` when
/// `open` is not there or never closes. A character inside a string
/// literal is the string's.
fn enclosed(text: &str, open: char, close: char) -> Option<(&str, &str)> {
    let body = text.trim_start().strip_prefix(open)?;
    let mut depth = 1usize;
    let mut quoted = false;
    for (at, c) in body.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if quoted {
            continue;
        } else if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some((&body[..at], &body[at + c.len_utf8()..]));
            }
        }
    }
    None
}

/// Reads the calibration the committed baseline names ahead of its groups,
/// `"calibration": { "operation": "<group>/<name>", "planning_ms": <ms> }`,
/// or `None` when it names none.
pub fn calibration(text: &str) -> Result<Option<Calibration>, BaselineError> {
    let end = text
        .find("\"groups\":")
        .or_else(|| text.find(MACHINES))
        .unwrap_or(text.len());
    let Some(at) = text[..end].find(CALIBRATION) else {
        return Ok(None);
    };
    let refused = |why: &str| BaselineError::Calibration(why.to_string());
    let (object, _) = object_at(&text[at + CALIBRATION.len()..end])
        .ok_or_else(|| refused("no object follows `calibration`"))?;
    let operation = string_after(object, "\"operation\":")
        .filter(|operation| operation.contains('/'))
        .ok_or_else(|| refused("it names no operation as `<group>/<name>` in `operation`"))?;
    let planning_ms = number_after(object, "planning_ms")
        .filter(|ms| *ms > 0.0)
        .ok_or_else(|| refused("it gives no `planning_ms` above 0"))?;
    Ok(Some(Calibration {
        operation,
        planning_ms,
    }))
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

/// The window MS-04 sizes every head-side operation to, in milliseconds:
/// under its low end a mean is mostly the timer and the harness, over its high
/// end the suite cannot run ten interleaved rounds inside the paired job's
/// fifteen minutes. A property of the runner a job lands on, so it is held
/// there, by `bench-check --sizes`, and never in `cargo test`.
pub const SIZE_WINDOW_MS: (f64, f64) = (20.0, 300.0);

/// Which side of a comparison an operation's figure was measured on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeSide {
    /// The build under test, which the window holds.
    Head,
    /// The planning baseline a factor is taken against, whose operations may
    /// run longer: the factor is the point.
    Baseline,
}

/// Where one operation's mean sits against [`SIZE_WINDOW_MS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// Inside the window.
    Inside,
    /// Under it: a failure on either side.
    Under,
    /// Over it on the head side: a failure.
    Over,
    /// Outside it, by the owner's word in `baseline.json`: reported.
    NamedOutside,
    /// Over it on the baseline side, which may run longer: reported.
    BaselineLonger,
}

impl Size {
    /// Whether `--sizes` fails on it.
    #[must_use]
    pub fn fails(self) -> bool {
        matches!(self, Size::Under | Size::Over)
    }
}

/// Where a mean of `millis` sits for an operation measured on `side`, named
/// outside the window by the owner or not.
#[must_use]
pub fn size_of(millis: f64, side: SizeSide, named_outside: bool) -> Size {
    let (low, high) = SIZE_WINDOW_MS;
    if (low..=high).contains(&millis) {
        return Size::Inside;
    }
    if named_outside {
        return Size::NamedOutside;
    }
    if millis > high {
        return match side {
            SizeSide::Head => Size::Over,
            SizeSide::Baseline => Size::BaselineLonger,
        };
    }
    Size::Under
}

/// An operation's throughput in its committed unit, from one iteration's
/// mean: pages a second, megapixels or megabytes a second, or outlines a
/// second; `None` for an operation whose unit is `none`, or a mean of nothing.
#[must_use]
pub fn throughput(unit: Unit, per_iteration: u64, nanos: f64) -> Option<String> {
    if nanos <= 0.0 || !nanos.is_finite() {
        return None;
    }
    let per_second = per_iteration as f64 / (nanos / 1_000_000_000.0);
    Some(match unit {
        Unit::Pages => format!("{per_second:.1} pages/s"),
        Unit::Pixels => format!("{:.2} Mpixel/s", per_second / 1_000_000.0),
        Unit::Bytes => format!("{:.2} MB/s", per_second / 1_000_000.0),
        Unit::Outlines => format!("{per_second:.0} outlines/s"),
        Unit::None => return None,
    })
}

/// What `--sizes` makes of a run: a line for every operation outside the
/// window, and the ones that fail it.
#[derive(Debug, Default, PartialEq)]
pub struct Sizes {
    /// Each operation outside the window, with its mean and what it is.
    pub lines: Vec<String>,
    /// The ids that fail the check.
    pub failing: Vec<String>,
}

/// Holds each measured operation of `now` that `groups` lists to the window,
/// as a head-side figure.
#[must_use]
pub fn sizes(groups: &[Group], now: &BTreeMap<String, f64>) -> Sizes {
    let mut out = Sizes::default();
    for group in groups {
        for listed in &group.operations {
            let id = group.id(&listed.name);
            let Some(nanos) = now.get(&id) else {
                continue;
            };
            let millis = nanos / 1_000_000.0;
            let size = size_of(millis, SizeSide::Head, listed.outside_window.is_some());
            let said = match size {
                Size::Inside => continue,
                Size::Under => "under the window".to_string(),
                Size::Over => "over the window".to_string(),
                Size::NamedOutside => format!(
                    "outside the window by the owner's word: {}",
                    listed.outside_window.as_deref().unwrap_or_default()
                ),
                Size::BaselineLonger => "a baseline side, which may run longer".to_string(),
            };
            out.lines.push(format!("  {id}: {millis:.3} ms, {said}"));
            if size.fails() {
                out.failing.push(id);
            }
        }
    }
    out
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

/// The host's CPU, as `/proc/cpuinfo`'s first `model name` line gives it.
#[must_use]
pub fn host_model(cpuinfo: &str) -> Option<String> {
    cpuinfo.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "model name").then(|| value.trim().to_string())
    })
}

/// Whether a host whose `/proc/cpuinfo` reads `cpuinfo` may be held to
/// `machine`'s figures: any host for a fleet, and for a pinned machine only
/// the host its entry names, so a run that lands elsewhere is refused by name
/// rather than judged against figures it did not set.
pub fn host_allows(machine: &Machine, cpuinfo: Option<&str>) -> Result<(), String> {
    let Some(pinned) = &machine.pinned else {
        return Ok(());
    };
    match cpuinfo.and_then(host_model) {
        Some(host) if host == pinned.cpu => Ok(()),
        Some(host) => Err(format!(
            "this host is `{host}`, and `{}` is `{}` held at {}: its figures are \
             that machine's, so this run is refused rather than judged against them",
            machine.name, pinned.cpu, pinned.held
        )),
        None => Err(format!(
            "this host's model name cannot be read from /proc/cpuinfo, and `{}` is \
             `{}` held at {}: refused rather than judged against figures another \
             host set",
            machine.name, pinned.cpu, pinned.held
        )),
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
/// group file, the file defines the group's operations, and the baseline lists
/// them and records a figure for each or names it unmeasured — and nothing
/// else makes them agree. So the agreement is checked where a build can fail
/// on it.
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

        // Each operation's gate, tier P's Gate column: one for every
        // operation, each relation to another of its group, and a
        // calibrated one against a calibration the file names.
        let calibration = calibration(&text)
            .map_err(|error| problems.push(error.to_string()))
            .ok()
            .flatten();
        let mut calibrated = Vec::new();
        for list in lists {
            for operation in &list.operations {
                let Some(gate) = &operation.gate else {
                    problems.push(format!(
                        "the `{}` list gives `{}` no gate; every operation is held \
                         against the planning baseline as tier P's Gate column holds \
                         it, by a floor, held or a calibrated target, or the paired \
                         run has nothing to hold it to",
                        list.name, operation.name
                    ));
                    continue;
                };
                if let Against::Calibrated(_) = gate.against {
                    calibrated.push(list.id(&operation.name));
                }
                for relation in &gate.relations {
                    if relation.to == operation.name {
                        problems.push(format!(
                            "the `{}` list relates `{}` to itself",
                            list.name, operation.name
                        ));
                    } else if !list
                        .operations
                        .iter()
                        .any(|other| other.name == relation.to)
                    {
                        problems.push(format!(
                            "the `{}` list relates `{}` to `{}`, which it does not hold; \
                             a relation compares two operations of one group, timed by \
                             one build in one round",
                            list.name, operation.name, relation.to
                        ));
                    }
                }
            }
        }
        match &calibration {
            None => {
                for id in &calibrated {
                    problems.push(format!(
                        "{BASELINE_PATH} calibrates `{id}` and names no calibration \
                         ahead of its groups, so the target has nothing to be a \
                         fraction of"
                    ));
                }
            }
            Some(calibration) => {
                let own = lists.iter().find_map(|list| {
                    list.operations
                        .iter()
                        .find(|operation| list.id(&operation.name) == calibration.operation)
                });
                match own.map(|operation| operation.gate.as_ref()) {
                    None => problems.push(format!(
                        "{BASELINE_PATH} calibrates against `{}`, which no list holds",
                        calibration.operation
                    )),
                    Some(Some(Gate {
                        against: Against::Calibrated(_),
                        ..
                    })) => problems.push(format!(
                        "{BASELINE_PATH} calibrates against `{}`, whose own gate is \
                         calibrated; the calibration is the planning baseline's to \
                         time, so it is held against that baseline",
                        calibration.operation
                    )),
                    Some(_) => {}
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
/// How many rounds a paired run times each operation in on each side: a sign
/// test of eight slower rounds in ten needs ten.
pub const ROUNDS: usize = 10;

/// How many of an operation's rounds must be slower, its median past its
/// band besides, before the pair fails it.
pub const SLOWER_TO_FAIL: usize = 8;

/// How many samples each round takes of an operation: criterion's fewest,
/// as the paired job's `--sample-size 10` asks.
pub const ROUND_SAMPLES: usize = 10;

/// One side of a pair: each round's per-call figure for each operation, in
/// nanoseconds, round 1 first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Side {
    /// One map a round, from an operation's id to its figure.
    pub rounds: Vec<BTreeMap<String, f64>>,
}

/// One operation both sides timed, and its ten paired ratios.
#[derive(Clone, Debug, PartialEq)]
pub struct Pair {
    /// The operation.
    pub id: String,
    /// The head's figure over the base's, round by round.
    pub ratios: Vec<f64>,
    /// The band its median is held to, in per cent, or `None` before one is
    /// set: then it is reported and not judged.
    pub band_percent: Option<f64>,
}

/// The median of `ratios`: the mean of the two middle ones of an even count,
/// so of the fifth and the sixth of ten; 1 for none.
fn median(ratios: &[f64]) -> f64 {
    let mut sorted = ratios.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    if sorted.is_empty() {
        return 1.0;
    }
    if sorted.len() % 2 == 0 {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

impl Pair {
    /// The median of the ratios: the mean of the two middle ones of an even
    /// count, so of the fifth and the sixth of ten.
    #[must_use]
    pub fn median(&self) -> f64 {
        median(&self.ratios)
    }

    /// How many rounds the head was slower in: a ratio above 1.
    #[must_use]
    pub fn slower(&self) -> usize {
        self.ratios.iter().filter(|ratio| **ratio > 1.0).count()
    }

    /// Whether the pair fails the operation: its median above 1 + its band
    /// **and** at least [`SLOWER_TO_FAIL`] of its rounds slower. Each half
    /// covers what the other cannot: the band keeps an A/A pair green, and
    /// the sign test asks that a slowdown show in eight rounds of ten rather
    /// than in a few far ones.
    #[must_use]
    pub fn fails(&self) -> bool {
        self.band_percent.is_some_and(|band| {
            self.median() > 1.0 + band / 100.0 && self.slower() >= SLOWER_TO_FAIL
        })
    }
}

/// What pairing two sides finds: every operation both timed, and the ids one
/// side timed alone, which are reported and not judged.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paired {
    /// Each operation both sides timed, in id order.
    pub pairs: Vec<Pair>,
    /// Ids the base timed and the head did not.
    pub base_only: Vec<String>,
    /// Ids the head timed and the base did not.
    pub head_only: Vec<String>,
}

/// Pairs `base` with `head` round by round, each operation's band read from
/// `bands`. Refused when a side has other than [`ROUNDS`] rounds, or an
/// operation a side timed is missing from one of that side's rounds: a
/// missing round is a pair the sign test cannot count.
pub fn pair(base: &Side, head: &Side, bands: &BTreeMap<String, f64>) -> Result<Paired, String> {
    let base_ids = timed(base, "base")?;
    let head_ids = timed(head, "head")?;
    let mut paired = Paired::default();
    for id in &head_ids {
        if !base_ids.contains(id) {
            paired.head_only.push(id.clone());
            continue;
        }
        let ratios = (0..ROUNDS)
            .map(|round| head.rounds[round][id] / base.rounds[round][id])
            .collect();
        paired.pairs.push(Pair {
            id: id.clone(),
            ratios,
            band_percent: bands.get(id).copied(),
        });
    }
    paired.base_only = base_ids
        .into_iter()
        .filter(|id| !head_ids.contains(id))
        .collect();
    Ok(paired)
}

/// Every id `side` timed, sorted, once its rounds are held whole: refused
/// when it has other than [`ROUNDS`] rounds, or an id it timed is missing
/// from one of them, since a missing round is a pair the sign test cannot
/// count. `which` names the side in the refusal.
fn timed(side: &Side, which: &str) -> Result<Vec<String>, String> {
    if side.rounds.len() != ROUNDS {
        return Err(format!(
            "the {which} holds {} rounds, and a pair needs {ROUNDS}",
            side.rounds.len()
        ));
    }
    let mut all: Vec<String> = side
        .rounds
        .iter()
        .flat_map(|round| round.keys().cloned())
        .collect();
    all.sort();
    all.dedup();
    for id in &all {
        if let Some(at) = side.rounds.iter().position(|round| !round.contains_key(id)) {
            return Err(format!(
                "the {which} timed `{id}` and has no figure for it in round {}: a \
                 missing round is refused, never paired around",
                at + 1
            ));
        }
    }
    Ok(all)
}

/// The name the planning baseline goes by in a report and a refusal.
pub const PLANNING: &str = "planning baseline `61adcde`";

/// One gate of one operation, judged on the ten rounds of the head and the
/// planning baseline: each round's ratio, the bound it is held to and the
/// band beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Judged {
    /// The operation whose gate it is.
    pub id: String,
    /// The gate, as the report names it.
    pub gate: String,
    /// Round by round, the operation's per-call time over the time its gate
    /// holds it against.
    pub ratios: Vec<f64>,
    /// The most a round's ratio may be.
    pub bound: f64,
    /// The band its median is held to beyond the bound, in per cent, or
    /// `None` before one is set: then it is reported and not judged. A
    /// relation's is the wider of its two operations' bands, and it has
    /// none until both have one.
    pub band_percent: Option<f64>,
}

impl Judged {
    /// The median of the ratios, as [`Pair::median`] takes it.
    #[must_use]
    pub fn median(&self) -> f64 {
        median(&self.ratios)
    }

    /// How many rounds lie beyond the bound.
    #[must_use]
    pub fn beyond(&self) -> usize {
        self.ratios
            .iter()
            .filter(|ratio| **ratio > self.bound)
            .count()
    }

    /// Tier P's own reading of the gate: the median beyond the bound, with
    /// no band and no sign test. Reported beside [`Judged::fails`]: with
    /// the two builds equally fast, a floor of 1 read this way fails about
    /// one run in two.
    #[must_use]
    pub fn beyond_as_written(&self) -> bool {
        self.median() > self.bound
    }

    /// Whether the gate fails the run: the regression pair's rule held to
    /// the bound in place of 1, its median beyond the bound by more than
    /// its band and at least [`SLOWER_TO_FAIL`] of its rounds beyond it.
    #[must_use]
    pub fn fails(&self) -> bool {
        self.band_percent.is_some_and(|band| {
            self.median() > self.bound * (1.0 + band / 100.0) && self.beyond() >= SLOWER_TO_FAIL
        })
    }
}

/// What holding a head to the planning baseline finds: each gate judged,
/// and the ids the planning baseline timed that no gate reads, which are
/// reported.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Planned {
    /// Every gate of every operation the head's lists hold, in their order.
    pub judged: Vec<Judged>,
    /// Ids the planning baseline timed that no floor and no calibration
    /// reads.
    pub unread: Vec<String>,
}

/// Holds the head's rounds to the planning baseline's by each operation's
/// gate in `lists`, the head's: a floor or a held operation by its ratio to
/// the planning baseline's figure for it, a calibrated one by its ratio to
/// the planning baseline's figure for `calibration` in the same round, and
/// each relation by the head's two figures in the same round.
///
/// Refused when a side's rounds are not whole, an operation the lists hold
/// has no gate or no figure on the head, a floor or a held operation has
/// none on the planning baseline, or a calibrated one has no calibration
/// to read: a gate the data says can be judged and the rounds cannot is a
/// fault, never a pass.
pub fn against_planning(
    planning: &Side,
    head: &Side,
    lists: &[Group],
    calibration: Option<&Calibration>,
) -> Result<Planned, String> {
    let planning_ids = timed(planning, PLANNING)?;
    let head_ids = timed(head, "head")?;
    let ratios = |numerator: (&Side, &str), denominator: (&Side, &str)| -> Vec<f64> {
        (0..ROUNDS)
            .map(|round| {
                numerator.0.rounds[round][numerator.1] / denominator.0.rounds[round][denominator.1]
            })
            .collect()
    };
    let mut planned = Planned::default();
    let mut read: Vec<String> = Vec::new();
    for list in lists {
        for operation in &list.operations {
            let id = list.id(&operation.name);
            let gate = operation
                .gate
                .as_ref()
                .ok_or_else(|| format!("the head's lists give `{id}` no gate"))?;
            if !head_ids.contains(&id) {
                return Err(format!(
                    "the head did not time `{id}`, which its lists gate: an operation \
                     the gate data says can be judged and the rounds cannot is refused"
                ));
            }
            // What the planning baseline's side of it is: its own figure
            // for the operation, or its calibration's.
            let (against, bound, named) = match gate.against {
                Against::Floor(floor) => (id.clone(), 1.0 / floor, format!("floor {floor}")),
                Against::Held => (id.clone(), 1.0, "held".to_string()),
                Against::Calibrated(ms) => {
                    let calibration = calibration.ok_or_else(|| {
                        format!("`{id}` is calibrated, and the head's lists name no calibration")
                    })?;
                    (
                        calibration.operation.clone(),
                        ms / calibration.planning_ms,
                        format!(
                            "calibrated at {ms} ms of `{}`'s {} ms",
                            calibration.operation, calibration.planning_ms
                        ),
                    )
                }
            };
            if !planning_ids.contains(&against) {
                return Err(if against == id {
                    format!(
                        "`{id}` is held against the {PLANNING}, which did not time it: \
                         its mapping onto that revision is missing, and an operation \
                         whose gate names a baseline side is refused without one"
                    )
                } else {
                    format!("`{id}` is calibrated against `{against}`, which the {PLANNING} did not time")
                });
            }
            planned.judged.push(Judged {
                id: id.clone(),
                gate: named,
                ratios: ratios((head, &id), (planning, &against)),
                bound,
                band_percent: operation.paired_band,
            });
            read.push(against);
            for relation in &gate.relations {
                let to = list.id(&relation.to);
                let Some(other) = list.operations.iter().find(|o| o.name == relation.to) else {
                    return Err(format!(
                        "`{id}` is related to `{to}`, which the head's lists do not hold"
                    ));
                };
                if !head_ids.contains(&to) {
                    return Err(format!(
                        "`{id}` is related to `{to}`, which the head did not time"
                    ));
                }
                planned.judged.push(Judged {
                    id: id.clone(),
                    gate: match relation.bound {
                        Bound::AtMost(times) => format!("at most {times} x `{to}`"),
                        Bound::SpeedupAtLeast(times) => {
                            format!("at least {times} x as fast as `{to}`")
                        }
                    },
                    ratios: ratios((head, &id), (head, &to)),
                    bound: relation.bound.limit(),
                    band_percent: match (operation.paired_band, other.paired_band) {
                        (Some(one), Some(two)) => Some(one.max(two)),
                        _ => None,
                    },
                });
            }
        }
    }
    planned.unread = planning_ids
        .into_iter()
        .filter(|id| !read.contains(id))
        .collect();
    Ok(planned)
}

/// `bench-check --paired --planning PLANNING HEAD`: the head's rounds held
/// to the planning baseline's by each operation's gate in the head's
/// lists, every reading printed. A gate with its band fails the run as the
/// regression pair's rule says; one without is reported. Nothing is timed a
/// second time.
fn run_planning(planning: &Path, head: &Path) -> Result<(), String> {
    let (planning_side, _) = read_side(planning)?;
    let (head_side, head_lists) = read_side(head)?;
    let lists_path = head.join("baseline.json");
    let text = std::fs::read_to_string(&lists_path)
        .map_err(|error| format!("{}: {error}", lists_path.display()))?;
    let calibration = calibration(&text).map_err(|error| error.to_string())?;
    let planned = against_planning(
        &planning_side,
        &head_side,
        &head_lists,
        calibration.as_ref(),
    )?;
    println!(
        "bench-check --paired --planning: {} gates over {ROUNDS} rounds, {} against the \
         {PLANNING} in {}",
        planned.judged.len(),
        head.display(),
        planning.display()
    );
    for judged in &planned.judged {
        let band = judged
            .band_percent
            .map_or("no band yet: reported".to_string(), |band| {
                format!("band {band} %")
            });
        println!(
            "  median {:.4} against {:.4}, {:2}/{ROUNDS} beyond, {} as written, {band}{}  \
             {}: {}",
            judged.median(),
            judged.bound,
            judged.beyond(),
            if judged.beyond_as_written() {
                "beyond"
            } else {
                "inside"
            },
            if judged.fails() { ", FAILS" } else { "" },
            judged.id,
            judged.gate
        );
    }
    for id in &planned.unread {
        println!("  the {PLANNING}'s alone, reported, not judged: {id}");
    }
    let failing: Vec<&Judged> = planned.judged.iter().filter(|j| j.fails()).collect();
    if failing.is_empty() {
        return Ok(());
    }
    let mut message = format!(
        "beyond its gate against the {PLANNING} by more than the band in at least \
         {SLOWER_TO_FAIL} of {ROUNDS} rounds:\n"
    );
    for judged in failing {
        message.push_str(&format!(
            "  {}, {}: median {:.4} against {:.4}, {}/{ROUNDS} rounds beyond, band {} %\n",
            judged.id,
            judged.gate,
            judged.median(),
            judged.bound,
            judged.beyond(),
            judged.band_percent.unwrap_or_default()
        ));
    }
    Err(message)
}

/// The ten iteration counts and the samples of one `sample.json`, refused
/// unless it holds [`ROUND_SAMPLES`] samples taken flat, each of one count.
fn check_samples(file: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(file)
        .map_err(|error| format!("cannot read {}: {error}", file.display()))?;
    let iters = text
        .find("\"iters\":")
        .and_then(|at| {
            let rest = &text[at..];
            let open = rest.find('[')?;
            let close = rest[open..].find(']')? + open;
            Some(
                rest[open + 1..close]
                    .split(',')
                    .map(|n| n.trim().parse::<f64>())
                    .collect::<Result<Vec<_>, _>>(),
            )
        })
        .and_then(Result::ok)
        .ok_or_else(|| format!("{} holds no readable `iters`", file.display()))?;
    if iters.len() != ROUND_SAMPLES || iters.iter().any(|n| *n != iters[0]) {
        return Err(format!(
            "{} holds {} samples of {:?} iterations; a round takes {ROUND_SAMPLES} \
             samples of one count, flat, and a round sampled otherwise is refused",
            file.display(),
            iters.len(),
            iters
        ));
    }
    Ok(())
}

/// Every operation one round of one build timed: each `<group>/<name>/new/`
/// under `dir`, its `estimates.json`'s mean over the `calls` its side's list
/// gives it, its `sample.json` checked.
fn read_round(dir: &Path, calls: &BTreeMap<String, u64>) -> Result<BTreeMap<String, f64>, String> {
    let mut found = BTreeMap::new();
    collect(dir, &mut Vec::new(), &mut found);
    let mut round = BTreeMap::new();
    for (id, mean) in found {
        let Some(calls) = calls.get(&id) else {
            return Err(format!(
                "{} times `{id}`, which its side's lists do not name, so a call's time \
                 cannot be read",
                dir.display()
            ));
        };
        check_samples(&dir.join(&id).join("new").join("sample.json"))?;
        round.insert(id, mean / *calls as f64);
    }
    Ok(round)
}

/// One side of a pair, read from `dir`: its lists, the `baseline.json` its
/// build committed, copied beside its rounds so that a call is that build's
/// call, and its rounds `round-01` to `round-10`, each the `CRITERION_HOME`
/// one round of that build wrote.
pub fn read_side(dir: &Path) -> Result<(Side, Vec<Group>), String> {
    let lists_path = dir.join("baseline.json");
    let text = std::fs::read_to_string(&lists_path).map_err(|error| {
        format!(
            "{}: {error}; each side carries its build's committed lists beside its \
             rounds, so that each figure is a call's time by that build's `calls`",
            lists_path.display()
        )
    })?;
    let lists = groups(&text, &lists_path.display().to_string()).map_err(|e| e.to_string())?;
    let calls: BTreeMap<String, u64> = lists
        .iter()
        .flat_map(|list| {
            list.operations
                .iter()
                .map(|operation| (list.id(&operation.name), operation.calls))
        })
        .collect();
    let mut side = Side::default();
    for round in 1..=ROUNDS {
        let round_dir = dir.join(format!("round-{round:02}"));
        if !round_dir.is_dir() {
            return Err(format!(
                "{} is missing; a side holds {ROUNDS} rounds, and a missing round is refused",
                round_dir.display()
            ));
        }
        side.rounds.push(read_round(&round_dir, &calls)?);
    }
    Ok((side, lists))
}

/// `bench-check --paired BASE HEAD`: the head's rounds against the base's,
/// each operation judged on its median paired ratio and the sign test, its
/// band read from the head's lists. Nothing is timed a second time.
fn run_paired(base: &Path, head: &Path) -> Result<(), String> {
    let (base_side, _) = read_side(base)?;
    let (head_side, head_lists) = read_side(head)?;
    let bands: BTreeMap<String, f64> = head_lists
        .iter()
        .flat_map(|list| {
            list.operations
                .iter()
                .filter_map(|operation| Some((list.id(&operation.name), operation.paired_band?)))
        })
        .collect();
    let paired = pair(&base_side, &head_side, &bands)?;
    println!(
        "bench-check --paired: {} operations over {ROUNDS} rounds, {} against {}",
        paired.pairs.len(),
        head.display(),
        base.display()
    );
    for pair in &paired.pairs {
        let band = pair
            .band_percent
            .map_or("no band yet: reported".to_string(), |band| {
                format!("band {band} %")
            });
        println!(
            "  {:+7.2} % median, {:2}/{ROUNDS} slower, {band}{}  {}",
            (pair.median() - 1.0) * 100.0,
            pair.slower(),
            if pair.fails() { ", FAILS" } else { "" },
            pair.id
        );
    }
    for id in &paired.head_only {
        println!("  the head's alone, reported, not judged: {id}");
    }
    for id in &paired.base_only {
        println!("  the base's alone, reported, not judged: {id}");
    }
    let failing: Vec<&Pair> = paired.pairs.iter().filter(|pair| pair.fails()).collect();
    if failing.is_empty() {
        return Ok(());
    }
    let mut message = format!(
        "slower than the base beyond the band in at least {SLOWER_TO_FAIL} of {ROUNDS} rounds:\n"
    );
    for pair in failing {
        message.push_str(&format!(
            "  {}: median {:+.2} %, {}/{ROUNDS} rounds slower, band {} %\n",
            pair.id,
            (pair.median() - 1.0) * 100.0,
            pair.slower(),
            pair.band_percent.unwrap_or_default()
        ));
    }
    Err(message)
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut machine_name = None;
    // `None` holds no window; `Some(false)` fails outside it and
    // `Some(true)` only reports, the dispatch the set is sized from.
    let mut sizes_mode: Option<bool> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--machine" => {
                index += 1;
                machine_name = args.get(index).cloned();
            }
            "--sizes" => {
                sizes_mode = Some(args.get(index + 1).map(String::as_str) == Some("report"));
                if sizes_mode == Some(true) {
                    index += 1;
                }
            }
            "--paired" => {
                let planning = args.get(index + 1).map(String::as_str) == Some("--planning");
                let at = index + 1 + usize::from(planning);
                let (Some(base), Some(head)) = (args.get(at), args.get(at + 1)) else {
                    return Err(if planning {
                        "bench-check --paired --planning needs PLANNING HEAD".to_string()
                    } else {
                        "bench-check --paired needs BASE HEAD".to_string()
                    });
                };
                if args.len() != at + 2 || machine_name.is_some() {
                    return Err(if planning {
                        "bench-check --paired --planning PLANNING HEAD takes nothing else"
                            .to_string()
                    } else {
                        "bench-check --paired BASE HEAD takes nothing else".to_string()
                    });
                }
                return if planning {
                    run_planning(Path::new(base), Path::new(head))
                } else {
                    run_paired(Path::new(base), Path::new(head))
                };
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

    // Each operation's throughput in the unit its list commits it to, beside
    // its mean: a figure a reader can hold against the engine's other claims,
    // where milliseconds alone say how long the input took and not how much.
    let listed = groups(&text, BASELINE_PATH).map_err(|e| e.to_string())?;
    println!("bench-check: throughput, from each operation's committed unit");
    for group in &listed {
        for operation in &group.operations {
            let id = group.id(&operation.name);
            if let Some(nanos) = now.get(&id) {
                let rate = throughput(operation.unit, operation.per_iteration, *nanos)
                    .map(|rate| format!(", {rate}"))
                    .unwrap_or_default();
                println!("  {id}: {:.3} ms{rate}", nanos / 1_000_000.0);
            }
        }
    }
    let sized = sizes_mode.map(|report| (report, sizes(&listed, &now)));
    if let Some((report, held)) = &sized {
        let (low, high) = SIZE_WINDOW_MS;
        println!(
            "bench-check: sizes, every head-side operation held to {low}-{high} ms{}",
            if *report {
                ", reported and not failed"
            } else {
                ""
            }
        );
        if held.lines.is_empty() {
            println!("  every operation measured is inside the window");
        }
        for line in &held.lines {
            println!("{line}");
        }
    }
    let size_failure = match &sized {
        Some((false, held)) if !held.failing.is_empty() => Some(format!(
            "outside the {}-{} ms window, and not named outside it by the owner: {}",
            SIZE_WINDOW_MS.0,
            SIZE_WINDOW_MS.1,
            held.failing.join(", ")
        )),
        _ => None,
    };

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
        return size_failure.map_or(Ok(()), Err);
    };

    // Nothing is spawned: the host's model name is read from the file itself.
    host_allows(
        machine,
        std::fs::read_to_string("/proc/cpuinfo").ok().as_deref(),
    )?;
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
        return size_failure.map_or(Ok(()), Err);
    }
    let mut message = String::from("slower than the baseline by more than the band:\n");
    if let Some(sizes) = &size_failure {
        message = format!("{sizes}\n{message}");
    }
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

    /// A group list of three operations, one of them named outside the window.
    const SIZED: &str = r#"{
      "groups": [
        { "group": "write", "operations": [
          { "name": "rewrite of 100 pages", "unit": "pages", "per_iteration": 100, "calls": 1 },
          { "name": "inflate 4 MB", "unit": "bytes", "per_iteration": 4000000, "calls": 1 },
          { "name": "rewrite of 3000 pages", "unit": "pages", "per_iteration": 3000, "calls": 1,
            "outside_window": "tier P states the input" }
        ] }
      ]
    }"#;

    fn sized(millis: [f64; 3]) -> Sizes {
        let listed = groups(SIZED, "x").expect("the list parses");
        let now: BTreeMap<String, f64> = [
            "write/rewrite of 100 pages",
            "write/inflate 4 MB",
            "write/rewrite of 3000 pages",
        ]
        .into_iter()
        .zip(millis)
        .map(|(id, ms)| (id.to_string(), ms * 1_000_000.0))
        .collect();
        sizes(&listed, &now)
    }

    #[test]
    fn throughput_is_read_from_the_committed_unit() {
        assert_eq!(
            throughput(Unit::Pages, 100, 50_000_000.0).as_deref(),
            Some("2000.0 pages/s")
        );
        assert_eq!(
            throughput(Unit::Pixels, 2_000_000, 1_000_000_000.0).as_deref(),
            Some("2.00 Mpixel/s")
        );
        assert_eq!(
            throughput(Unit::Bytes, 4_000_000, 500_000_000.0).as_deref(),
            Some("8.00 MB/s")
        );
        assert_eq!(
            throughput(Unit::Outlines, 300, 100_000_000.0).as_deref(),
            Some("3000 outlines/s")
        );
        assert_eq!(throughput(Unit::None, 1, 1_000_000.0), None);
        assert_eq!(throughput(Unit::Pages, 1, 0.0), None);
        let listed = groups(SIZED, "x").expect("the list parses");
        assert_eq!(listed[0].operations[1].unit, Unit::Bytes);
        assert_eq!(listed[0].operations[1].per_iteration, 4_000_000);
    }

    #[test]
    fn an_operation_under_twenty_ms_fails_the_size_check() {
        assert_eq!(size_of(19.9, SizeSide::Head, false), Size::Under);
        assert_eq!(size_of(20.0, SizeSide::Head, false), Size::Inside);
        let held = sized([12.0, 150.0, 140.0]);
        assert_eq!(held.failing, ["write/rewrite of 100 pages"]);
        assert_eq!(
            held.lines,
            ["  write/rewrite of 100 pages: 12.000 ms, under the window"]
        );
        // The baseline side may run longer, never shorter.
        assert_eq!(size_of(5.0, SizeSide::Baseline, false), Size::Under);
    }

    #[test]
    fn an_operation_over_three_hundred_ms_fails_it() {
        assert_eq!(size_of(300.1, SizeSide::Head, false), Size::Over);
        assert_eq!(size_of(300.0, SizeSide::Head, false), Size::Inside);
        let held = sized([150.0, 301.0, 140.0]);
        assert_eq!(held.failing, ["write/inflate 4 MB"]);
    }

    #[test]
    fn a_baseline_side_may_run_longer() {
        let size = size_of(5480.0, SizeSide::Baseline, false);
        assert_eq!(size, Size::BaselineLonger);
        assert!(!size.fails());
        assert!(size_of(5480.0, SizeSide::Head, false).fails());
    }

    #[test]
    fn an_operation_named_outside_the_window_is_reported_not_failed() {
        let held = sized([150.0, 140.0, 5480.0]);
        assert!(held.failing.is_empty(), "{:?}", held.failing);
        assert_eq!(
            held.lines,
            [
                "  write/rewrite of 3000 pages: 5480.000 ms, outside the window by the \
              owner's word: tier P states the input"
            ]
        );
        assert_eq!(size_of(5480.0, SizeSide::Head, true), Size::NamedOutside);
        assert!(!Size::NamedOutside.fails());
    }

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
    /// committed baseline, the group files and their `[[bench]]` entries, held
    /// to each other as they stand.
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
  "note": ["Two groups whose files, lists, counts and gates agree."],
  "groups": [
    {
      "group": "open",
      "operations": [
        { "name": "open a 3-page document", "unit": "bytes",
          "per_iteration": 10985, "calls": 1, "gate": { "floor": 1 } }
      ]
    },
    {
      "group": "render",
      "operations": [
        { "name": "render text at 150 dpi", "unit": "pixels",
          "per_iteration": 2176200, "calls": 1, "gate": { "held": true } },
        { "name": "render a full-page axial shading at 150 dpi", "unit": "pixels",
          "per_iteration": 2176200, "calls": 1,
          "gate": { "floor": 1, "relations": [
            { "to": "render text at 150 dpi", "at_most": 25 } ] } }
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

    /// The fixture's machine, pinned to one host held still.
    fn pinned_lists() -> String {
        LISTS.replace(
            "      \"recorded\": \"5 September 2026\",\n",
            "      \"recorded\": \"5 September 2026\",\n      \
             \"cpu\": \"AMD EPYC 7763 64-Core Processor\",\n      \
             \"held\": \"2.45 GHz, cores 2 and 3 isolated\",\n",
        )
    }

    /// `/proc/cpuinfo` as a host with `model` reports its first processor.
    fn cpuinfo(model: &str) -> String {
        format!(
            "processor\t: 0\nvendor_id\t: AuthenticAMD\ncpu family\t: 25\n\
             model\t\t: 1\nmodel name\t: {model}\nstepping\t: 1\n\n\
             processor\t: 1\nmodel name\t: {model}\n"
        )
    }

    /// A pinned machine's figures are one host's, so a run that lands on
    /// another is refused by both names, and so is one on a host whose model
    /// cannot be read.
    #[test]
    fn a_host_that_is_not_the_pinned_machine_is_refused() {
        let machines = parse(&pinned_lists(), BASELINE_PATH).expect("it parses");
        let machine = &machines[0];
        assert_eq!(
            machine.pinned,
            Some(Pinned {
                cpu: "AMD EPYC 7763 64-Core Processor".to_string(),
                held: "2.45 GHz, cores 2 and 3 isolated".to_string(),
            })
        );
        let other = cpuinfo("Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz");
        let refused = host_allows(machine, Some(&other)).expect_err("another host");
        for words in [
            "`Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz`",
            "`AMD EPYC 7763 64-Core Processor` held at 2.45 GHz",
            "refused rather than judged",
        ] {
            assert!(refused.contains(words), "no `{words}` in: {refused}");
        }
        let unread = host_allows(machine, None).expect_err("an unread host");
        assert!(unread.contains("cannot be read"), "{unread}");
        assert!(
            host_allows(machine, Some("processor\t: 0\n")).is_err(),
            "a cpuinfo with no model name is no host"
        );
    }

    /// The pinned machine's own host compares, and a fleet — the fixture's,
    /// and the committed `ubuntu-latest`, which names no model — compares on
    /// any host, exactly as before a machine could be pinned.
    #[test]
    fn the_pinned_machine_compares() {
        let machines = parse(&pinned_lists(), BASELINE_PATH).expect("it parses");
        let same = cpuinfo("AMD EPYC 7763 64-Core Processor");
        assert_eq!(
            host_model(&same).as_deref(),
            Some("AMD EPYC 7763 64-Core Processor")
        );
        assert_eq!(host_allows(&machines[0], Some(&same)), Ok(()));

        let other = cpuinfo("Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz");
        let fleet = &parse(LISTS, BASELINE_PATH).expect("it parses")[0];
        assert_eq!(fleet.pinned, None);
        assert_eq!(host_allows(fleet, Some(&other)), Ok(()));
        assert_eq!(host_allows(fleet, None), Ok(()));

        let text =
            std::fs::read_to_string(repo_root().join(BASELINE_PATH)).expect("the committed file");
        let committed = parse(&text, BASELINE_PATH).expect("it parses");
        let ubuntu = committed
            .iter()
            .find(|machine| machine.name == "ubuntu-latest")
            .expect("the committed fleet");
        assert_eq!(ubuntu.pinned, None, "`ubuntu-latest` is a fleet");
        assert_eq!(host_allows(ubuntu, Some(&other)), Ok(()));
        assert_eq!(host_allows(ubuntu, None), Ok(()));
    }

    /// A pin is a host and how it is held, both ahead of the operations: half
    /// of one, or either after them, is refused rather than read as a fleet.
    #[test]
    fn a_pin_with_half_its_fields_is_refused() {
        let held_only =
            pinned_lists().replace("      \"cpu\": \"AMD EPYC 7763 64-Core Processor\",\n", "");
        assert!(matches!(
            parse(&held_only, BASELINE_PATH),
            Err(BaselineError::Field { key: "cpu", .. })
        ));
        let cpu_only = pinned_lists().replace(
            "      \"held\": \"2.45 GHz, cores 2 and 3 isolated\",\n",
            "",
        );
        assert!(matches!(
            parse(&cpu_only, BASELINE_PATH),
            Err(BaselineError::Field { key: "held", .. })
        ));
        let late = LISTS.replace(
            "\"band_percent\": 11.0 }\n      ]\n    }\n  ]",
            "\"band_percent\": 11.0 }\n      ],\n      \"cpu\": \"AMD EPYC 7763 64-Core \
             Processor\",\n      \"held\": \"2.45 GHz\"\n    }\n  ]",
        );
        assert_ne!(late, LISTS, "the fixture moved");
        assert!(matches!(
            parse(&late, BASELINE_PATH),
            Err(BaselineError::Misplaced { key: "cpu", .. })
        ));
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
                             \"unit\": \"bytes\",\n          \"per_iteration\": 10985, \"calls\": 1, \
                             \"gate\": { \"floor\": 1 } }\n      ]",
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

    /// One side's lists: a banded operation, an unbanded one, and one whose
    /// iteration makes `calls` calls.
    fn paired_lists(calls: u64) -> String {
        format!(
            "{{\n  \"groups\": [\n    {{ \"group\": \"render\", \"operations\": [\n      \
             {{ \"name\": \"render text at 150 dpi\", \"unit\": \"pixels\",\n        \
             \"per_iteration\": 2176200, \"calls\": 1,\n        \
             \"paired\": {{ \"spread_percent\": 3.0, \"band_percent\": 5 }} }},\n      \
             {{ \"name\": \"the text page at 72 dpi\", \"unit\": \"pixels\",\n        \
             \"per_iteration\": 2504950, \"calls\": {calls},\n        \
             \"paired\": {{ \"spread_percent\": 3.0, \"band_percent\": 5 }} }},\n      \
             {{ \"name\": \"5000 paths at 150 dpi\", \"unit\": \"pixels\",\n        \
             \"per_iteration\": 2176200, \"calls\": 1 }}\n    ] }}\n  ],\n  \
             \"machines\": []\n}}\n"
        )
    }

    /// A side written out as the paired job keeps one: its lists, and ten
    /// round directories whose `<group>/<name>/new/` hold the
    /// `estimates.json` and `sample.json` criterion writes, each operation's
    /// mean in round r being `nanos(id, r)`, `None` leaving it out.
    fn stage_side(
        tag: &str,
        lists: &str,
        ids: &[&str],
        nanos: impl Fn(&str, usize) -> Option<f64>,
    ) -> PathBuf {
        let root = std::env::temp_dir().join(format!("tinker-paired-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).expect("a directory");
        std::fs::write(root.join("baseline.json"), lists).expect("a file");
        for round in 1..=ROUNDS {
            for id in ids {
                let Some(mean) = nanos(id, round) else {
                    continue;
                };
                let new = root.join(format!("round-{round:02}")).join(id).join("new");
                std::fs::create_dir_all(&new).expect("a directory");
                std::fs::write(
                    new.join("estimates.json"),
                    format!(
                        "{{\"mean\":{{\"confidence_interval\":{{\"confidence_level\":0.95,\
                         \"lower_bound\":{mean},\"upper_bound\":{mean}}},\
                         \"point_estimate\":{mean},\"standard_error\":0.0}}}}"
                    ),
                )
                .expect("a file");
                let iters = ["1.0"; ROUND_SAMPLES].join(",");
                std::fs::write(
                    new.join("sample.json"),
                    format!("{{\"sampling_mode\":\"Flat\",\"iters\":[{iters}],\"times\":[]}}"),
                )
                .expect("a file");
            }
            std::fs::create_dir_all(root.join(format!("round-{round:02}"))).expect("a directory");
        }
        root
    }

    const TEXT_150: &str = "render/render text at 150 dpi";
    const TEXT_72: &str = "render/the text page at 72 dpi";
    const PATHS: &str = "render/5000 paths at 150 dpi";

    /// The base's figure for every operation in every round.
    fn base_nanos(id: &str, _round: usize) -> Option<f64> {
        Some(if id == TEXT_72 {
            2_000_000.0
        } else {
            1_000_000.0
        })
    }

    /// `bench-check --paired` over a staged base and head, and what pairing
    /// them finds, the stage removed.
    fn compared(
        tag: &str,
        head: impl Fn(&str, usize) -> Option<f64>,
    ) -> (Result<(), String>, Result<Paired, String>) {
        let ids = [TEXT_150, TEXT_72, PATHS];
        let base = stage_side(&format!("{tag}-base"), &paired_lists(1), &ids, base_nanos);
        let head = stage_side(&format!("{tag}-head"), &paired_lists(1), &ids, head);
        let verdict = run_paired(&base, &head);
        let paired = read_side(&base).and_then(|(base_side, _)| {
            let (head_side, lists) = read_side(&head)?;
            let bands = lists
                .iter()
                .flat_map(|list| {
                    list.operations.iter().filter_map(|operation| {
                        Some((list.id(&operation.name), operation.paired_band?))
                    })
                })
                .collect();
            pair(&base_side, &head_side, &bands)
        });
        std::fs::remove_dir_all(&base).ok();
        std::fs::remove_dir_all(&head).ok();
        (verdict, paired)
    }

    fn found<'a>(paired: &'a Paired, id: &str) -> &'a Pair {
        paired
            .pairs
            .iter()
            .find(|pair| pair.id == id)
            .expect("the pair")
    }

    /// Every round's head at 1.10 times its base, against a band of 5 %:
    /// a median of 1.10 and ten slower rounds, so the operation fails, by
    /// name, and the band-less one beside it is reported and never failed.
    #[test]
    fn a_ten_percent_slowdown_is_caught() {
        let (verdict, paired) = compared("slowdown", |id, round| {
            base_nanos(id, round).map(|nanos| nanos * 1.10)
        });
        let message = verdict.expect_err("a 10 % slowdown is caught");
        assert!(
            message.contains(TEXT_150) && message.contains(TEXT_72),
            "{message}"
        );
        assert!(
            !message.contains(PATHS),
            "an operation with no band is not judged: {message}"
        );
        let paired = paired.expect("the sides pair");
        let pair = found(&paired, TEXT_150);
        assert!((pair.median() - 1.10).abs() < 1e-9, "{}", pair.median());
        assert_eq!(pair.slower(), 10);
        assert!(pair.fails());
        assert!(!found(&paired, PATHS).fails(), "no band, no verdict");
    }

    /// Two builds of one revision: the rounds part by a few hundredths
    /// either way, the median sits at 1, and nothing fails.
    #[test]
    fn an_a_a_pair_passes() {
        let (verdict, paired) = compared("a-a", |id, round| {
            let wobble = if round % 2 == 0 { 1.01 } else { 0.99 };
            base_nanos(id, round).map(|nanos| nanos * wobble)
        });
        assert_eq!(verdict, Ok(()));
        let paired = paired.expect("the sides pair");
        assert_eq!(paired.pairs.len(), 3);
        for pair in &paired.pairs {
            assert!(
                (pair.median() - 1.0).abs() < 1e-9,
                "{}: {}",
                pair.id,
                pair.median()
            );
            assert_eq!(pair.slower(), 5, "{}", pair.id);
            assert!(!pair.fails(), "{}", pair.id);
        }
    }

    /// Seven rounds 20 % slower and three faster: a median far past the band,
    /// and a slowdown the sign test does not see in eight rounds of ten, so
    /// the operation passes; an eighth slower round fails it.
    #[test]
    fn seven_slower_rounds_of_ten_pass_the_sign_test() {
        let rounds = |slower: usize| {
            move |id: &str, round: usize| {
                let factor = if round <= slower { 1.20 } else { 0.95 };
                base_nanos(id, round).map(|nanos| nanos * factor)
            }
        };
        let (verdict, paired) = compared("seven", rounds(7));
        assert_eq!(verdict, Ok(()));
        let pair = found(paired.as_ref().expect("the sides pair"), TEXT_150).clone();
        assert!(pair.median() > 1.05, "{}", pair.median());
        assert_eq!(pair.slower(), 7);
        assert!(!pair.fails());

        let (verdict, _) = compared("eight", rounds(8));
        assert!(verdict
            .expect_err("eight slower rounds fail")
            .contains(TEXT_150));
    }

    /// Every round slower, by 4 %, against a band of 5 %: the sign test sees
    /// ten slower rounds and the median stays inside the band, so it passes.
    #[test]
    fn a_median_inside_the_band_passes_however_many_rounds_are_slower() {
        let (verdict, paired) = compared("inside", |id, round| {
            base_nanos(id, round).map(|nanos| nanos * 1.04)
        });
        assert_eq!(verdict, Ok(()));
        let pair = found(paired.as_ref().expect("the sides pair"), TEXT_150).clone();
        assert_eq!(pair.slower(), 10);
        assert!((pair.median() - 1.04).abs() < 1e-9);
        assert!(!pair.fails());
    }

    /// A round is never paired around: a side whose operation is missing
    /// from one round, a side missing a round's directory, a side of nine
    /// rounds, and a round sampled other than ten flat samples are each
    /// refused by what is missing.
    #[test]
    fn a_missing_round_is_refused() {
        let (verdict, _) = compared("gap", |id, round| {
            (!(id == TEXT_150 && round == 7))
                .then(|| base_nanos(id, round))
                .flatten()
        });
        let message = verdict.expect_err("a missing round");
        assert!(
            message.contains(&format!(
                "timed `{TEXT_150}` and has no figure for it in round 7"
            )),
            "{message}"
        );

        let ids = [TEXT_150];
        let side = stage_side("no-dir", &paired_lists(1), &ids, base_nanos);
        std::fs::remove_dir_all(side.join("round-04")).expect("the round");
        let refused = read_side(&side).expect_err("a round's directory missing");
        assert!(refused.contains("round-04 is missing"), "{refused}");
        std::fs::remove_dir_all(&side).ok();

        let nine = Side {
            rounds: vec![BTreeMap::from([(TEXT_150.to_string(), 1.0)]); ROUNDS - 1],
        };
        let ten = Side {
            rounds: vec![BTreeMap::from([(TEXT_150.to_string(), 1.0)]); ROUNDS],
        };
        let refused = pair(&nine, &ten, &BTreeMap::new()).expect_err("nine rounds");
        assert!(refused.contains("holds 9 rounds"), "{refused}");

        let side = stage_side("nine-samples", &paired_lists(1), &ids, base_nanos);
        let sample = side
            .join("round-02")
            .join(TEXT_150)
            .join("new")
            .join("sample.json");
        let iters = ["1.0"; ROUND_SAMPLES - 1].join(",");
        std::fs::write(&sample, format!("{{\"iters\":[{iters}],\"times\":[]}}")).expect("a file");
        let refused = read_side(&side).expect_err("nine samples");
        assert!(refused.contains("holds 9 samples"), "{refused}");
        std::fs::remove_dir_all(&side).ok();
    }

    /// Each side's figure is a call's time by that side's own `calls`, so an
    /// iteration that makes twice the calls on the head moves nothing; and
    /// an operation one side timed alone is reported, not judged.
    #[test]
    fn a_call_is_each_sides_own_and_a_one_sided_operation_is_reported() {
        let base = stage_side(
            "calls-base",
            &paired_lists(1),
            &[TEXT_150, TEXT_72, PATHS],
            base_nanos,
        );
        let head = stage_side(
            "calls-head",
            &paired_lists(2),
            &[TEXT_150, TEXT_72],
            |id, round| {
                base_nanos(id, round).map(|nanos| if id == TEXT_72 { nanos * 2.0 } else { nanos })
            },
        );
        let (base_side, _) = read_side(&base).expect("the base");
        let (head_side, _) = read_side(&head).expect("the head");
        let paired = pair(&base_side, &head_side, &BTreeMap::new()).expect("they pair");
        assert!((found(&paired, TEXT_72).median() - 1.0).abs() < 1e-9);
        assert_eq!(paired.base_only, [PATHS.to_string()]);
        assert!(paired.head_only.is_empty());
        assert_eq!(run_paired(&base, &head), Ok(()));
        std::fs::remove_dir_all(&base).ok();
        std::fs::remove_dir_all(&head).ok();
    }

    // ---- the gates against the planning baseline ------------------------------

    /// A head's lists for the planning pair: a floor of 2 and a floor of 1
    /// related to it, a held operation and a four-band one held to a
    /// speed-up over it, a target in milliseconds and one as a rate, the
    /// rate related to an operation with a band of its own, and the
    /// calibration, at the 751 ms tier P rescaled its 37.6 from.
    const GATED: &str = r#"{
  "calibration": { "operation": "calibration/text of 100 dense pages",
                   "planning_ms": 751 },
  "groups": [
    { "group": "open", "operations": [
      { "name": "every page's size, 3000 pages", "unit": "pages",
        "per_iteration": 3000, "calls": 1, "gate": { "floor": 2 },
        "paired": { "spread_percent": 3.0, "band_percent": 5 } },
      { "name": "every page's size, 10000 pages", "unit": "pages",
        "per_iteration": 10000, "calls": 1,
        "gate": { "floor": 1, "relations": [
          { "to": "every page's size, 3000 pages", "at_most": 3.6 } ] },
        "paired": { "spread_percent": 6.0, "band_percent": 10 } } ] },
    { "group": "render", "operations": [
      { "name": "the text page at 300 dpi", "unit": "pixels",
        "per_iteration": 8702320, "calls": 1, "gate": { "held": true },
        "paired": { "spread_percent": 3.0, "band_percent": 5 } },
      { "name": "the 300-dpi text page in 4 bands on 4 threads", "unit": "pixels",
        "per_iteration": 8702320, "calls": 1,
        "gate": { "floor": 1, "relations": [
          { "to": "the text page at 300 dpi", "speedup_at_least": 4 } ] },
        "paired": { "spread_percent": 3.0, "band_percent": 5 } },
      { "name": "an ICC-based CMYK image to sRGB", "unit": "pixels",
        "per_iteration": 543740, "calls": 1, "gate": { "calibrated_ms": 0.5 },
        "paired": { "spread_percent": 3.0, "band_percent": 5 } } ] },
    { "group": "decode", "operations": [
      { "name": "inflate page-like data", "unit": "bytes",
        "per_iteration": 6528600, "calls": 1, "gate": { "floor": 1 },
        "paired": { "spread_percent": 3.0, "band_percent": 5 } },
      { "name": "LZW decode page-like data", "unit": "bytes",
        "per_iteration": 32643000, "calls": 5,
        "gate": { "relations": [ { "to": "inflate page-like data", "at_most": 2 } ],
                  "calibrated_per_second": 200000000 } } ] },
    { "group": "calibration", "operations": [
      { "name": "text of 100 dense pages", "unit": "pages",
        "per_iteration": 100, "calls": 1, "gate": { "floor": 1 },
        "paired": { "spread_percent": 3.0, "band_percent": 5 } } ] }
  ],
  "machines": []
}"#;

    const SIZES_3000: &str = "open/every page's size, 3000 pages";
    const SIZES_10000: &str = "open/every page's size, 10000 pages";
    const PAGE_300: &str = "render/the text page at 300 dpi";
    const BANDS: &str = "render/the 300-dpi text page in 4 bands on 4 threads";
    const CMYK: &str = "render/an ICC-based CMYK image to sRGB";
    const INFLATE: &str = "decode/inflate page-like data";
    const LZW: &str = "decode/LZW decode page-like data";
    const CALIBRATION_100: &str = "calibration/text of 100 dense pages";

    /// What the planning baseline times: every operation but the two it
    /// leaves to calibration.
    const PLANNED_IDS: [&str; 6] = [
        SIZES_3000,
        SIZES_10000,
        PAGE_300,
        BANDS,
        INFLATE,
        CALIBRATION_100,
    ];

    /// What the head times: all of it.
    const HEAD_IDS: [&str; 8] = [
        SIZES_3000,
        SIZES_10000,
        PAGE_300,
        BANDS,
        CMYK,
        INFLATE,
        LZW,
        CALIBRATION_100,
    ];

    /// The planning baseline's per-call figure for each operation, in every
    /// round.
    fn planning_figure(id: &str, _round: usize) -> f64 {
        match id {
            SIZES_3000 => 2_000_000.0,
            SIZES_10000 => 10_000_000.0,
            CALIBRATION_100 => 1_502_000.0,
            _ => 1_000_000.0,
        }
    }

    /// A head's per-call figure for each operation that holds every gate:
    /// the floor of 2 met exactly, the relations at their bounds and the
    /// calibrated targets inside theirs.
    fn holding(id: &str, _round: usize) -> f64 {
        match id {
            SIZES_3000 | SIZES_10000 | PAGE_300 | INFLATE => 1_000_000.0,
            BANDS => 250_000.0,
            CMYK => 1_000.0,
            LZW => 60_000.0,
            _ => 1_502_000.0,
        }
    }

    /// Ten rounds of `ids`, each operation's figure in round r `figure(id,
    /// r)`.
    fn side_of(ids: &[&str], figure: impl Fn(&str, usize) -> f64) -> Side {
        Side {
            rounds: (1..=ROUNDS)
                .map(|round| {
                    ids.iter()
                        .map(|id| (id.to_string(), figure(id, round)))
                        .collect()
                })
                .collect(),
        }
    }

    /// The head `head` held against the planning baseline by [`GATED`].
    fn planned_with(
        planning: impl Fn(&str, usize) -> f64,
        head: impl Fn(&str, usize) -> f64,
    ) -> Result<Planned, String> {
        let lists = groups(GATED, "x").expect("the lists parse");
        let calibration = calibration(GATED).expect("it reads").expect("one is named");
        against_planning(
            &side_of(&PLANNED_IDS, planning),
            &side_of(&HEAD_IDS, head),
            &lists,
            Some(&calibration),
        )
    }

    /// The head `head` against [`planning_figure`], every gate held but
    /// what `changed` returns a figure for.
    fn planned(changed: impl Fn(&str, usize) -> Option<f64>) -> Planned {
        planned_with(planning_figure, |id, round| {
            changed(id, round).unwrap_or_else(|| holding(id, round))
        })
        .expect("the sides are judged")
    }

    /// The one gate of `id` whose name starts with `kind`.
    fn gate_of<'a>(planned: &'a Planned, id: &str, kind: &str) -> &'a Judged {
        let found: Vec<&Judged> = planned
            .judged
            .iter()
            .filter(|judged| judged.id == id && judged.gate.starts_with(kind))
            .collect();
        assert_eq!(found.len(), 1, "{id}, {kind}: {:?}", planned.judged);
        found[0]
    }

    /// The figures that hold every gate hold every gate, by both readings,
    /// and what the planning baseline timed for no gate is reported.
    #[test]
    fn a_head_that_holds_every_gate_passes_both_readings() {
        let planned = planned(|_, _| None);
        assert_eq!(planned.judged.len(), 11, "{:?}", planned.judged);
        for judged in &planned.judged {
            assert!(!judged.fails() && !judged.beyond_as_written(), "{judged:?}");
        }
        assert!(planned.unread.is_empty(), "{:?}", planned.unread);
    }

    /// A floor f holds the head's per-call time over the planning
    /// baseline's to 1 ÷ f: a floor of 2 fails 0.55 against a band of 5 %
    /// and holds 0.5. Between 1 ÷ f and its band tier P's own reading is
    /// beyond and the gate holds; and the sign test asks for eight rounds
    /// of ten beyond it. Held is a floor of 1.
    #[test]
    fn a_floor_fails_a_ratio_above_its_inverse() {
        let at = |figure: f64| planned(move |id, _| (id == SIZES_3000).then_some(figure));
        let fails = at(1_100_000.0);
        let floor = gate_of(&fails, SIZES_3000, "floor 2");
        assert_eq!(floor.bound, 0.5);
        assert!((floor.median() - 0.55).abs() < 1e-12);
        assert_eq!(floor.beyond(), 10);
        assert!(floor.fails() && floor.beyond_as_written());

        let met = at(1_000_000.0);
        let floor = gate_of(&met, SIZES_3000, "floor 2");
        assert_eq!(floor.median(), 0.5);
        assert!(!floor.fails() && !floor.beyond_as_written());

        let inside = at(1_030_000.0);
        let floor = gate_of(&inside, SIZES_3000, "floor 2");
        assert!(floor.beyond_as_written(), "0.515 is beyond 0.5 as written");
        assert!(!floor.fails(), "and inside its band of 5 %");

        let rounds = |slower: usize| {
            planned(move |id, round| {
                (id == SIZES_3000).then_some(if round <= slower {
                    1_200_000.0
                } else {
                    800_000.0
                })
            })
        };
        let seven = rounds(7);
        let floor = gate_of(&seven, SIZES_3000, "floor 2");
        assert!((floor.median() - 0.6).abs() < 1e-12);
        assert_eq!(floor.beyond(), 7);
        assert!(
            !floor.fails(),
            "seven rounds of ten beyond pass the sign test"
        );
        assert!(gate_of(&rounds(8), SIZES_3000, "floor 2").fails());

        let held = |figure: f64| planned(move |id, _| (id == PAGE_300).then_some(figure));
        let slower = held(1_100_000.0);
        let gate = gate_of(&slower, PAGE_300, "held");
        assert_eq!(gate.bound, 1.0);
        assert!(gate.fails(), "held is a floor of 1");
        assert!(!gate_of(&held(1_040_000.0), PAGE_300, "held").fails());

        // A floored operation the planning baseline did not time is a fault
        // of the mapping, refused rather than reported.
        let refused = against_planning(
            &side_of(&PLANNED_IDS[1..], planning_figure),
            &side_of(&HEAD_IDS, holding),
            &groups(GATED, "x").expect("the lists parse"),
            calibration(GATED).expect("it reads").as_ref(),
        )
        .expect_err("no baseline side");
        assert!(
            refused.contains(&format!("`{SIZES_3000}` is held against the {PLANNING}")),
            "{refused}"
        );
    }

    /// A calibrated target is the head's per-call time over the planning
    /// baseline's figure for the calibration in the same round, held to the
    /// target over the calibration's planning figure: 0.5 ms of a 751 ms
    /// calibration reads as at most 1/1 502 of it, whatever the machine's
    /// own figure for the calibration is that round. A target stated as a
    /// rate is the time one call takes at it. And the committed file states
    /// tier P's own calibration and targets.
    #[test]
    fn a_calibrated_target_is_held_as_a_fraction_of_the_calibration_operation() {
        let held = planned(|_, _| None);
        let cmyk = gate_of(&held, CMYK, "calibrated");
        assert_eq!(cmyk.bound, 1.0 / 1502.0);
        assert_eq!(cmyk.median(), 1.0 / 1502.0);
        assert!(!cmyk.fails() && !cmyk.beyond_as_written());

        // The machine slows by the round, and the head with it: each round
        // is read against its own calibration, so every round is at the
        // bound.
        let drifting = |calibration_by_round: bool| {
            planned_with(
                move |id, round| match id {
                    CALIBRATION_100 if calibration_by_round => 1_502_000.0 * round as f64,
                    _ => planning_figure(id, round),
                },
                move |id, round| match id {
                    CMYK => 1_000.0 * round as f64,
                    _ => holding(id, round),
                },
            )
            .expect("the sides are judged")
        };
        let same_round = drifting(true);
        let cmyk = gate_of(&same_round, CMYK, "calibrated");
        assert_eq!(cmyk.beyond(), 0, "{:?}", cmyk.ratios);
        assert!(!cmyk.fails());
        let round_one = drifting(false);
        let cmyk = gate_of(&round_one, CMYK, "calibrated");
        assert_eq!(cmyk.beyond(), 9, "{:?}", cmyk.ratios);
        assert!(cmyk.fails(), "a calibration that stood still would fail it");

        let slower = planned(|id, _| (id == CMYK).then_some(1_073.0));
        assert!(gate_of(&slower, CMYK, "calibrated").fails());

        // 200 MB/s over one call's 6 528 600 bytes is 32.643 ms.
        let lzw = gate_of(&held, LZW, "calibrated");
        assert!((lzw.bound - 32.643 / 751.0).abs() < 1e-15, "{}", lzw.bound);

        let refused = against_planning(
            &side_of(&PLANNED_IDS[..5], planning_figure),
            &side_of(&HEAD_IDS, holding),
            &groups(GATED, "x").expect("the lists parse"),
            calibration(GATED).expect("it reads").as_ref(),
        )
        .expect_err("no calibration timed");
        assert!(
            refused.contains(&format!(
                "`{CMYK}` is calibrated against `{CALIBRATION_100}`, which the {PLANNING} \
                 did not time"
            )),
            "{refused}"
        );

        let text =
            std::fs::read_to_string(repo_root().join(BASELINE_PATH)).expect("the committed file");
        assert_eq!(
            calibration(&text).expect("it reads"),
            Some(Calibration {
                operation: "calibration/text of 5 dense pages".to_string(),
                planning_ms: 37.6,
            })
        );
        let lists = groups(&text, BASELINE_PATH).expect("it parses");
        let against = |group: &str, name: &str| {
            lists
                .iter()
                .find(|list| list.name == group)
                .and_then(|list| list.operations.iter().find(|o| o.name == name))
                .and_then(|operation| operation.gate.as_ref())
                .map(|gate| gate.against)
        };
        assert_eq!(
            against(
                "render",
                "a quarter-size ICC-based CMYK image to sRGB at 300 dpi"
            ),
            Some(Against::Calibrated(5.4))
        );
        let Some(Against::Calibrated(lzw)) = against("decode", "LZW decode page-like data") else {
            panic!("LZW is calibrated");
        };
        assert!((lzw - 32.643).abs() < 1e-9, "{lzw}");
    }

    /// A relation holds the head's per-call times of two operations of a
    /// group, in one round, to its bound: 10 000 pages at most 3.6 times
    /// 3 000, which fails past 3.6 by the wider of the two bands; four
    /// bands at least 4 times as fast as one thread; and one whose
    /// operations do not both carry a band is reported.
    #[test]
    fn a_relation_fails_when_its_bound_is_crossed() {
        let at = |figure: f64| planned(move |id, _| (id == SIZES_10000).then_some(figure));
        let crossed = at(4_000_000.0);
        let relation = gate_of(&crossed, SIZES_10000, "at most 3.6");
        assert_eq!(relation.bound, 3.6);
        assert_eq!(relation.band_percent, Some(10.0), "the wider of 5 and 10");
        assert!(relation.fails() && relation.beyond_as_written());
        let within = at(3_800_000.0);
        let relation = gate_of(&within, SIZES_10000, "at most 3.6");
        assert!(relation.beyond_as_written() && !relation.fails());
        assert!(!gate_of(&at(3_600_000.0), SIZES_10000, "at most 3.6").beyond_as_written());
        // The floor beside it reads the planning baseline's 10 000 pages,
        // which the relation does not.
        assert!(!gate_of(&crossed, SIZES_10000, "floor 1").fails());

        let bands = |figure: f64| planned(move |id, _| (id == BANDS).then_some(figure));
        let slow = bands(280_000.0);
        let speedup = gate_of(&slow, BANDS, "at least 4 x as fast");
        assert_eq!(speedup.bound, 0.25);
        assert!(speedup.fails(), "3.57 times as fast is not 4");
        assert!(!gate_of(&bands(255_000.0), BANDS, "at least 4 x as fast").fails());

        let unbanded = planned(|id, _| (id == LZW).then_some(3_000_000.0));
        let relation = gate_of(&unbanded, LZW, "at most 2");
        assert_eq!(relation.band_percent, None);
        assert!(relation.beyond_as_written() && !relation.fails());
    }

    /// The gates read off the rounds on disk, as `bench-check --paired
    /// --planning` reads them: each figure a call's, so 10 000 pages at
    /// four calls an iteration against 3 000 at twelve fails a relation an
    /// iteration's figures would pass; the run fails by the gate's name and
    /// passes a head that holds every gate.
    #[test]
    fn the_planning_pair_is_read_from_its_rounds() {
        let lists = GATED
            .replace(
                "\"per_iteration\": 3000, \"calls\": 1",
                "\"per_iteration\": 36000, \"calls\": 12",
            )
            .replace(
                "\"per_iteration\": 10000, \"calls\": 1",
                "\"per_iteration\": 40000, \"calls\": 4",
            );
        let calls = |id: &str| match id {
            SIZES_3000 => 12.0,
            SIZES_10000 => 4.0,
            LZW => 5.0,
            _ => 1.0,
        };
        let planning = stage_side("gates-planning", &lists, &PLANNED_IDS, |id, round| {
            Some(planning_figure(id, round) * calls(id))
        });
        let stage_head = |tag: &str, per_call: fn(&str, usize) -> f64| {
            stage_side(tag, &lists, &HEAD_IDS, move |id, round| {
                Some(per_call(id, round) * calls(id))
            })
        };
        let holds = stage_head("gates-holds", holding);
        let fails = stage_head("gates-fails", |id, round| {
            if id == SIZES_10000 {
                4_000_000.0
            } else {
                holding(id, round)
            }
        });
        let path = |dir: &Path| dir.display().to_string();
        let ran = |head: &Path| {
            run(
                &repo_root(),
                &[
                    "--paired".to_string(),
                    "--planning".to_string(),
                    path(&planning),
                    path(head),
                ],
            )
        };
        let held = ran(&holds);
        let failed = ran(&fails);
        let usage = run(
            &repo_root(),
            &[
                "--paired".to_string(),
                "--planning".to_string(),
                path(&planning),
            ],
        );
        for dir in [&planning, &holds, &fails] {
            std::fs::remove_dir_all(dir).ok();
        }
        assert_eq!(held, Ok(()));
        let message = failed.expect_err("4 x 3 000 pages a call is past 3.6");
        assert!(
            message.contains(&format!("{SIZES_10000}, at most 3.6 x `{SIZES_3000}`")),
            "{message}"
        );
        assert_eq!(message.matches("rounds beyond").count(), 1, "{message}");
        assert_eq!(
            usage,
            Err("bench-check --paired --planning needs PLANNING HEAD".to_string())
        );
    }

    /// Every operation carries a gate, so the paired run has something to
    /// hold each to, and one with none is refused by name.
    #[test]
    fn an_operation_with_no_gate_is_refused() {
        let mut tree = Tree::agreeing();
        tree.baseline = tree.baseline.replace(
            "\"calls\": 1, \"gate\": { \"held\": true } }",
            "\"calls\": 1 }",
        );
        let problems = tree.problems("ungated");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("the `render` list gives `render text at 150 dpi` no gate"),
            "{problems:?}"
        );
    }

    /// A relation compares two operations of one group, so one naming an
    /// operation its group does not hold, another group's included, or
    /// naming its own operation, is refused.
    #[test]
    fn a_relation_naming_an_unknown_operation_is_refused() {
        refuses(
            "relation",
            &[
                (
                    "an operation nobody lists",
                    |tree| {
                        tree.baseline = tree.baseline.replace(
                            "{ \"to\": \"render text at 150 dpi\"",
                            "{ \"to\": \"render nothing at all\"",
                        );
                    },
                    "relates `render a full-page axial shading at 150 dpi` to `render \
                     nothing at all`, which it does not hold",
                ),
                (
                    "another group's operation",
                    |tree| {
                        tree.baseline = tree.baseline.replace(
                            "{ \"to\": \"render text at 150 dpi\"",
                            "{ \"to\": \"open a 3-page document\"",
                        );
                    },
                    "to `open a 3-page document`, which it does not hold",
                ),
                (
                    "its own operation",
                    |tree| {
                        tree.baseline = tree.baseline.replace(
                            "{ \"to\": \"render text at 150 dpi\"",
                            "{ \"to\": \"render a full-page axial shading at 150 dpi\"",
                        );
                    },
                    "relates `render a full-page axial shading at 150 dpi` to itself",
                ),
            ],
        );
    }

    /// Each way a gate or the calibration can stand outside the format,
    /// refused by what is wrong with it.
    #[test]
    fn a_gate_outside_the_format_is_refused() {
        refuses(
            "gate-format",
            &[
                (
                    "a gate holding it no way",
                    |tree| {
                        tree.baseline = tree
                            .baseline
                            .replace("\"gate\": { \"held\": true }", "\"gate\": { }");
                    },
                    "names none of `floor`, `held`, `calibrated_ms`, `calibrated_per_second`",
                ),
                (
                    "a gate holding it two ways",
                    |tree| {
                        tree.baseline = tree.baseline.replace(
                            "\"gate\": { \"held\": true }",
                            "\"gate\": { \"held\": true, \"floor\": 2 }",
                        );
                    },
                    "names `floor` and `held`, and an operation is held one way",
                ),
                (
                    "a floor under 1",
                    |tree| {
                        tree.baseline = tree.baseline.replacen(
                            "\"gate\": { \"floor\": 1 }",
                            "\"gate\": { \"floor\": 0.5 }",
                            1,
                        );
                    },
                    "a floor of 0.5, under 1",
                ),
                (
                    "held other than true",
                    |tree| {
                        tree.baseline = tree.baseline.replace("\"held\": true", "\"held\": false");
                    },
                    "`held` is not `true`",
                ),
                (
                    "a relation with no bound",
                    |tree| {
                        tree.baseline = tree.baseline.replace(", \"at_most\": 25 }", " }");
                    },
                    "the relation to `render text at 150 dpi` names no bound above 0",
                ),
                (
                    "a relation with both bounds",
                    |tree| {
                        tree.baseline = tree.baseline.replace(
                            ", \"at_most\": 25 }",
                            ", \"at_most\": 25, \"speedup_at_least\": 2 }",
                        );
                    },
                    "names both `at_most` and `speedup_at_least`",
                ),
                (
                    "a rate for an operation whose unit counts nothing",
                    |tree| {
                        tree.baseline = tree.baseline.replace(
                            "\"unit\": \"bytes\",\n          \"per_iteration\": 10985, \
                             \"calls\": 1, \"gate\": { \"floor\": 1 }",
                            "\"unit\": \"none\",\n          \"per_iteration\": 1, \
                             \"calls\": 1, \"gate\": { \"calibrated_per_second\": 5 }",
                        );
                    },
                    "the unit `none` counts none",
                ),
                (
                    "a calibrated target with no calibration",
                    |tree| {
                        tree.baseline = tree.baseline.replacen(
                            "\"gate\": { \"floor\": 1 }",
                            "\"gate\": { \"calibrated_ms\": 5 }",
                            1,
                        );
                    },
                    "calibrates `open/open a 3-page document` and names no calibration",
                ),
                (
                    "a calibration no list holds",
                    |tree| {
                        tree.baseline = tree.baseline.replace(
                            "  \"groups\": [",
                            "  \"calibration\": { \"operation\": \"open/nothing\", \
                             \"planning_ms\": 37.6 },\n  \"groups\": [",
                        );
                    },
                    "calibrates against `open/nothing`, which no list holds",
                ),
                (
                    "a calibration calibrated against itself",
                    |tree| {
                        tree.baseline = tree
                            .baseline
                            .replace(
                                "  \"groups\": [",
                                "  \"calibration\": { \"operation\": \
                                 \"open/open a 3-page document\", \"planning_ms\": 37.6 },\n  \
                                 \"groups\": [",
                            )
                            .replacen(
                                "\"gate\": { \"floor\": 1 }",
                                "\"gate\": { \"calibrated_ms\": 5 }",
                                1,
                            );
                    },
                    "whose own gate is calibrated",
                ),
                (
                    "a calibration with no planning figure",
                    |tree| {
                        tree.baseline = tree.baseline.replace(
                            "  \"groups\": [",
                            "  \"calibration\": { \"operation\": \
                             \"open/open a 3-page document\" },\n  \"groups\": [",
                        );
                    },
                    "it gives no `planning_ms` above 0",
                ),
            ],
        );
    }
}
