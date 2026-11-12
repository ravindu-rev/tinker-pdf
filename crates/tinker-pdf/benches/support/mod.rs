//! Wall-clock benchmarks, deliberately outside the test suite: the harness
//! every group file beside this one shares.
//!
//! Clocks are banned from assertions in this repository — `bounds_ledger.rs`
//! bans `Instant::now` from itself, so that every bound is a property of an
//! input rather than of a machine. The cost of that rule is that every
//! performance number in `docs/` has been a one-time measurement with a date
//! beside it and nothing to stop it regressing.
//!
//! This is where the clocks live instead. Criterion is exempt tooling by name
//! (`Cargo.toml`'s header rule, `CONTRIBUTING.md`): it measures the engine and
//! is not part of it, nothing here ships, and `default-features = false` keeps
//! its plotting stack out of the tree. `cargo bench` runs on a schedule rather
//! than on the path between a push and a review, because a benchmark that
//! gates a pull request measures the runner it happened to land on.
//!
//! Regression comparison is the point, and it is criterion's to do:
//!
//! ```sh
//! cargo bench -p tinker-pdf -- --save-baseline before
//! # ... change something ...
//! cargo bench -p tinker-pdf -- --baseline before
//! ```
//!
//! # One file a group
//!
//! Each group of the benchmark set (`docs/design/performance.md`, MS-04) is
//! one file, `benches/<group>.rs`, and one `[[bench]]` in `Cargo.toml`, so
//! `cargo bench -p tinker-pdf --bench render` measures one group. A group
//! file opens one group,
//! `Group::new(c, "<group>")`, named for its group, so every id is
//! `<group>/<name>`, and declares each operation through
//! [`Group::operation`], or [`Group::calibrated`] for one the planning
//! baseline lacks, with what each iteration processes ([`Per`]) and its
//! routine on the [`Op`] it is handed. `baseline.json` lists the same names in the same order, each
//! with its unit, what an iteration covers (`per_iteration`) and how many
//! calls an iteration makes (`calls`). `cargo xtask baseline` holds the
//! `[[bench]]` entries, each file's names and its list to each other, and
//! refuses a group file that opens a
//! criterion group or calls `bench_function` itself, which count mode could
//! not run.
//!
//! # Names
//!
//! Criterion 0.5.1 records an operation under a directory made of its group
//! and its name, and makes each part filename-safe: it replaces `?`, `"`, `/`,
//! `\`, `*`, `<`, `>`, `:`, `|` and `^` with `_` and cuts the part at 64
//! bytes. A name either rule changes is recorded under a key the baseline does
//! not hold, reported as new and never held to its band. So every name is
//! printable ASCII, at most 64 bytes and free of those ten characters
//! (`PDF-A`, not `PDF/A`), and `cargo xtask baseline` refuses one that is not.
//! On Windows criterion also trims each part at its end and lowercases it,
//! and `cargo xtask bench-check` reads such a directory back as the name it
//! was made from.
//!
//! # Inputs are built when an operation needs them
//!
//! A group file builds each input in a `LazyCell` and checks it there, so an
//! input is built the first time an operation reads it. Criterion runs every
//! operation, so it builds every input, in the order the operations first
//! read them; count mode runs one operation, so it builds that operation's
//! inputs and no others.
//!
//! # Count mode
//!
//! A bench binary started with `TINKER_BENCH_COUNT=<group>/<name>` and
//! `TINKER_BENCH_ITERATIONS=<n>` runs that one operation's routine exactly
//! `n` times, outside criterion's sampler, prints
//! `count-mode: RAN <group>/<name> <n>` and exits. It reads no clock, so the
//! instructions the process executes are a property of the code and the
//! input, and the difference between a run of `n + 1` and a run of `n` is one
//! iteration's, with start-up and the inputs' construction cancelled. An
//! operation that is none of the binary's group's, or an iteration count that
//! is missing or no number, ends the process with a failure that names it.
//!
//! An operation whose iterations each take an input criterion leaves out of
//! its time ([`Op::iter_batched`]) counts that setup too, since nothing
//! outside the process can tell its instructions from the routine's.
//!
//! # The host
//!
//! When it measures a group, the harness writes [`HOST_RECORD`] into the
//! group's directory, beside the estimates criterion writes there: the
//! logical CPUs the process may use, as `std::thread::available_parallelism`
//! counts them, and the CPU as the OS names it, each left out where the host
//! gives none. The process that times the group writes it, so it is that
//! run's own, and `cargo xtask bench-check` prints it beside the group's
//! relations held inside one run and arms one named for a CPU count on it.
//!
//! # The planning baseline
//!
//! Tier P holds the head against `61adcde`, the planning baseline, and the
//! paired run builds this directory there in place of that revision's own
//! benches, with the head's `[[bench]]` entries, under
//! `--cfg tinker_planning_baseline`, which `Cargo.toml` declares to
//! `check-cfg`. Under it an operation's body calls what `61adcde`'s facade
//! has: where the head calls what that revision lacks, an item or arm
//! marked with the `cfg` computes the same thing from what it has. An
//! operation the planning baseline lacks altogether is declared with
//! [`Group::calibrated`], which that build leaves out: its gate in
//! `baseline.json` holds it against the calibration operation the planning
//! baseline times instead. `cargo xtask baseline` holds each declaration to
//! its gate.
//!
//! # The seven
//!
//! Seven operations, chosen because they are the ones the docs quote numbers
//! for: opening, rendering text, rendering a shading, extracting text, filling
//! anti-aliased paths, rewriting a document, and paginating a book. They were
//! one file, `benches/engine.rs`, until MS-04.2 moved each into its group's
//! file with its input and its call unchanged, the inputs into [`inputs`];
//! `baseline.json` keeps their figures under their group ids. The fill is
//! newer than the rest and was added for a reason worth keeping: **six of
//! these do not enter the scanline rasteriser**, so the roadmap's
//! vectorisation row — whose exit criterion named "the six benchmarks" — was
//! unmeasurable as written. See [`inputs::triangles`].
//!
//! # Why the allow
//!
//! Every group file includes this module and compiles its own copy, and each
//! uses a different subset of it — the open group needs no render, the render
//! group no committed book — so each reports the others' as dead.

#![allow(
    dead_code,
    reason = "shared by every group file; each uses a different subset"
)]

pub mod cli;
pub mod inputs;

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Duration;

use criterion::measurement::WallTime;
use criterion::{BatchSize, BenchmarkGroup, Criterion, SamplingMode, Throughput};
use tinker_pdf::Bitmap;

/// The sampling of every group but sign, fonts and scripts: ten samples, the
/// fewest criterion takes, after a second's warm-up, over three seconds.
///
/// Set in `criterion_group!`'s `config` and never on a group, because
/// criterion's command line overrides a config and a group's own settings
/// override the command line, so a run can sample differently without
/// editing a file. Flat sampling is the exception: criterion sets it only on
/// a group and no flag sets it, so [`Group::new`] sets it on each.
pub fn config() -> Criterion {
    Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
}

/// The sampling of the sign, fonts and scripts groups: thirty samples over
/// [`config`]'s times. Their operations are sized near 25 ms by their
/// targets, and at 100 ms or less thirty samples fit the three seconds ten
/// would take, so the closer reading costs nothing.
pub fn fine_config() -> Criterion {
    config().sample_size(30)
}

/// What one iteration of an operation processes, in the unit its list in
/// `baseline.json` gives it.
///
/// An operation whose input is none of these — a private-key operation, a
/// shaped paragraph — declares nothing and reports milliseconds alone.
#[derive(Clone, Copy, Debug)]
pub enum Per {
    /// Pages read, written or laid out.
    Pages(u64),
    /// Pixels: a render's bitmap's, or a decoded image's.
    Pixels(u64),
    /// Bytes, reported in decimal megabytes.
    Bytes(u64),
    /// Glyph outlines.
    Outlines(u64),
}

impl Per {
    /// The figure as criterion reports and records it.
    pub fn throughput(self) -> Throughput {
        match self {
            Per::Pages(n) | Per::Pixels(n) | Per::Outlines(n) => Throughput::Elements(n),
            Per::Bytes(n) => Throughput::BytesDecimal(n),
        }
    }
}

/// A render's unit: the pixels of the bitmap it draws.
pub fn pixels(bitmap: &Bitmap) -> Per {
    Per::Pixels(u64::from(bitmap.width) * u64::from(bitmap.height))
}

/// What count mode was asked for: the operation, and how many times to run
/// it. `None` when the binary was started without `TINKER_BENCH_COUNT`; an
/// iteration count that is missing or no number ends the process.
fn count_mode() -> Option<(String, u64)> {
    let operation = std::env::var("TINKER_BENCH_COUNT").ok()?;
    let Ok(iterations) = std::env::var("TINKER_BENCH_ITERATIONS") else {
        eprintln!("count-mode: TINKER_BENCH_COUNT={operation} needs TINKER_BENCH_ITERATIONS");
        std::process::exit(2);
    };
    let Ok(iterations) = iterations.parse() else {
        eprintln!("count-mode: TINKER_BENCH_ITERATIONS={iterations} is no count of iterations");
        std::process::exit(2);
    };
    Some((operation, iterations))
}

/// The record of the host a group's run is on, which the harness writes
/// into the group's directory under [`criterion_home`] and `cargo xtask
/// bench-check` reads there.
pub const HOST_RECORD: &str = "host.json";

/// Where criterion 0.5.1 writes a run's estimates, found as it finds it
/// (`criterion-0.5.1/src/lib.rs:135-145`): `CRITERION_HOME`, then
/// `CARGO_TARGET_DIR`'s `criterion/`, then the target directory this bench
/// was built in, which is the one criterion asks `cargo metadata` for; and
/// on `wasm32-wasip1`, where criterion can ask nothing, `target/criterion`.
fn criterion_home() -> PathBuf {
    if let Some(home) = std::env::var_os("CRITERION_HOME") {
        return PathBuf::from(home);
    }
    if let Some(target) = std::env::var_os("CARGO_TARGET_DIR") {
        return PathBuf::from(target).join("criterion");
    }
    if cfg!(target_os = "wasi") {
        return PathBuf::from("target/criterion");
    }
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .map_or_else(|| PathBuf::from("target"), Path::to_path_buf)
        .join("criterion")
}

/// The CPU as the host's OS names it: `/proc/cpuinfo`'s first model name,
/// or Windows' `PROCESSOR_IDENTIFIER`; `None` where neither is there.
fn host_cpu() -> Option<String> {
    let named = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|cpuinfo| {
            cpuinfo.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                (key.trim() == "model name").then(|| value.trim().to_string())
            })
        });
    named
        .or_else(|| std::env::var("PROCESSOR_IDENTIFIER").ok())
        .filter(|cpu| !cpu.trim().is_empty())
}

/// `text` as a JSON string.
fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The group `group`'s [`HOST_RECORD`], in its directory under
/// [`criterion_home`]: the logical CPUs this process may use, as
/// `std::thread::available_parallelism` counts them, and [`host_cpu`], each
/// left out where the host gives none.
fn record_host(group: &str) {
    let mut fields = Vec::new();
    if let Ok(cpus) = std::thread::available_parallelism() {
        fields.push(format!("\"cpus\": {cpus}"));
    }
    if let Some(cpu) = host_cpu() {
        fields.push(format!("\"cpu\": {}", json_string(&cpu)));
    }
    let dir = criterion_home().join(group);
    std::fs::create_dir_all(&dir)
        .and_then(|()| {
            std::fs::write(
                dir.join(HOST_RECORD),
                format!("{{ {} }}\n", fields.join(", ")),
            )
        })
        .unwrap_or_else(|error| panic!("the host record in {}: {error}", dir.display()));
}

/// One group of the set as its file declares it: criterion's group, flat
/// sampled, or in count mode the one operation `TINKER_BENCH_COUNT` names.
pub struct Group<'c> {
    id: String,
    mode: Mode<'c>,
    /// Whether an operation before this one declared a unit. Criterion keeps
    /// a group's throughput from one operation to the next and cannot clear
    /// it, so an operation that declared none after one that did would be
    /// reported in the earlier one's unit.
    united: bool,
}

enum Mode<'c> {
    Measure(BenchmarkGroup<'c, WallTime>),
    Count { operation: String, iterations: u64 },
}

impl<'c> Group<'c> {
    /// The group `id`: criterion's group, sampled flat, its host recorded
    /// beside its estimates, or in count mode the group's one operation asked
    /// for, run outside criterion.
    pub fn new(c: &'c mut Criterion, id: &str) -> Group<'c> {
        let mode = match count_mode() {
            Some((operation, iterations)) => Mode::Count {
                operation,
                iterations,
            },
            None => {
                record_host(id);
                let mut group = c.benchmark_group(id);
                group.sampling_mode(SamplingMode::Flat);
                Mode::Measure(group)
            }
        };
        Group {
            id: id.to_string(),
            mode,
            united: false,
        }
    }

    /// The operation `name`: `define` builds what it reads, declares its unit
    /// and runs its routine once through the [`Op`] it is handed, then checks
    /// what that run wrote. In count mode `define` is called for the
    /// operation asked for alone, and the process exits when it returns.
    pub fn operation(&mut self, name: &str, define: impl FnOnce(&mut Op<'_, 'c>)) {
        let id = format!("{}/{name}", self.id);
        let run = match &mut self.mode {
            Mode::Measure(group) => Run::Measure(group),
            Mode::Count {
                operation,
                iterations,
            } => {
                if *operation != id {
                    return;
                }
                Run::Count(*iterations)
            }
        };
        let mut op = Op {
            name,
            run,
            per: false,
            ran: false,
        };
        define(&mut op);
        assert!(op.ran, "{id} declares no routine");
        if let Run::Count(iterations) = op.run {
            println!("count-mode: RAN {id} {iterations}");
            std::process::exit(0);
        }
        assert!(
            op.per || !self.united,
            "{id} declares no unit after an operation that did, so criterion would report it \
             in that one's: declare its own, or move it before the first that has one"
        );
        self.united |= op.per;
    }

    /// The operation `name`, which the planning baseline lacks: declared as
    /// [`Group::operation`] declares one, except in the planning baseline's
    /// build (`tinker_planning_baseline`), which leaves it out, since its
    /// gate in `baseline.json` holds it against the calibration operation
    /// that build times instead.
    pub fn calibrated(&mut self, name: &str, define: impl FnOnce(&mut Op<'_, 'c>)) {
        if !cfg!(tinker_planning_baseline) {
            self.operation(name, define);
        }
    }

    /// The group's end: criterion's, or in count mode a failure naming the
    /// operation asked for, which is none of this group's.
    pub fn finish(self) {
        match self.mode {
            Mode::Measure(group) => group.finish(),
            Mode::Count { operation, .. } => {
                eprintln!(
                    "count-mode: {operation} is no operation of the {} group",
                    self.id
                );
                std::process::exit(2);
            }
        }
    }
}

/// One operation's run: criterion's sampler, or in count mode its routine
/// exactly n times, with no clock read.
pub struct Op<'g, 'c> {
    name: &'g str,
    run: Run<'g, 'c>,
    per: bool,
    ran: bool,
}

enum Run<'g, 'c> {
    Measure(&'g mut BenchmarkGroup<'c, WallTime>),
    Count(u64),
}

impl Op<'_, '_> {
    /// What one iteration processes, the unit criterion reports a rate in.
    pub fn per(&mut self, per: Per) {
        if let Run::Measure(group) = &mut self.run {
            group.throughput(per.throughput());
        }
        self.per = true;
    }

    /// `routine`, once an iteration.
    pub fn iter<R>(&mut self, mut routine: impl FnMut() -> R) {
        self.runs();
        match &mut self.run {
            Run::Measure(group) => {
                group.bench_function(self.name, |b| b.iter(&mut routine));
            }
            Run::Count(iterations) => {
                for _ in 0..*iterations {
                    black_box(routine());
                }
            }
        }
    }

    /// `routine` once an iteration on an input `setup` makes for it, which
    /// criterion leaves out of the time and count mode counts with it.
    pub fn iter_batched<I, R>(
        &mut self,
        mut setup: impl FnMut() -> I,
        mut routine: impl FnMut(I) -> R,
        size: BatchSize,
    ) {
        self.runs();
        match &mut self.run {
            Run::Measure(group) => {
                group.bench_function(self.name, |b| {
                    b.iter_batched(&mut setup, &mut routine, size);
                });
            }
            Run::Count(iterations) => {
                for _ in 0..*iterations {
                    black_box(routine(setup()));
                }
            }
        }
    }

    /// `routine`, once an iteration, timed as its own run plus the duration
    /// it answers: a figure part measured and part computed, as a link's
    /// latency and bandwidth are from the requests and bytes a run counted.
    /// Count mode runs it `n` times and reads no clock.
    pub fn iter_plus(&mut self, mut routine: impl FnMut() -> std::time::Duration) {
        self.runs();
        match &mut self.run {
            Run::Measure(group) => {
                group.bench_function(self.name, |b| {
                    b.iter_custom(|iterations| {
                        let mut total = std::time::Duration::ZERO;
                        for _ in 0..iterations {
                            let started = std::time::Instant::now();
                            let computed = black_box(routine());
                            total += started.elapsed() + computed;
                        }
                        total
                    });
                });
            }
            Run::Count(iterations) => {
                for _ in 0..*iterations {
                    black_box(routine());
                }
            }
        }
    }

    fn runs(&mut self) {
        assert!(!self.ran, "{} declares a second routine", self.name);
        self.ran = true;
    }
}
