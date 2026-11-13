//! The instruction-count gate: `cargo xtask bench-count`.
//!
//! A wall clock swings between two runs of identical code, which is why the
//! timed comparison holds each operation to a band measured over many runs.
//! An instruction count is a second clock: the benchmark set's count mode
//! (`benches/support/mod.rs`) runs one operation's routine a fixed number of
//! times and reads no clock, and cachegrind, the host instruction counter
//! ruling 23 admits on the bench machine (H-16), counts every instruction the
//! process executes. The counter executes the bench and judges no document
//! (ruling 13): the count is its figure, and holding a head's count to its
//! base's is this file's own.
//!
//! # A run
//!
//! The base and the head are each checked out as a detached worktree of this
//! repository under [`WORK_DIR`], named by its commit, and each builds its
//! release `tpdf` and the release bench binary of the group asked for into a
//! target directory of its own, so a CLI operation runs its own side's
//! `tpdf`; the bench binary with each of [`BENCH_FEATURES`] its facade's
//! manifest declares, as every run of the set is built, so an operation a
//! feature brings is in its group's binary on a side that has the feature. Each operation then runs in count mode under cachegrind
//! ([`COUNTER`]) at one iteration and at two, on each side. Every process is
//! traced, so a CLI operation's `tpdf` children count with the bench, and
//! each writes its own log; a run's count is the sum of its logs' `I refs`,
//! and the run of two less the run of one is one warm iteration's
//! instructions, start-up and the inputs' construction cancelled.
//!
//! # The verdict
//!
//! Each side's warm iteration is taken over the `calls` its own list gives
//! the operation, so a change to `calls` moves neither side, and the head
//! fails when its call counts more than [`RISE_PERCENT`] above its base's. A
//! fall is printed and never failed: the gate is one-sided, as every gate
//! here is. An operation the base's list does not hold is new at the head:
//! counted, printed and not judged. One the head's list no longer holds is
//! named and not counted.
//!
//! A count that did not happen is never a pass: a run that does not exit 0
//! and print its `RAN` line, a process whose log holds no count, and a warm
//! iteration that counts no instruction each fail the task by name.
//!
//! # A group, or a part of one, at a time
//!
//! Under cachegrind most of an operation's time is what its process does
//! besides its iterations, and a pair of the whole set takes hours
//! (`docs/verification.md`), so a run counts one group, or a part of one:
//! every N-th operation of the head's list from the K-th. The checkouts and
//! their builds stay under [`WORK_DIR`], so the next part of one pair builds
//! nothing again.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::bench::{self, Group, Listed};
use crate::json;

/// Where the checkouts, their builds and the counter's logs live, under the
/// repository root: a directory for each side's commit, and `logs/`.
pub const WORK_DIR: &str = "target/bench-count";

/// How far above its base's a head's call may count, in per cent, and pass.
pub const RISE_PERCENT: u64 = 1;

/// cachegrind as every count runs it, ahead of its log file and the program:
/// no cache simulated, so it counts instructions alone; its own output file
/// discarded, since the count is read from its log; and every child traced,
/// so a CLI operation's `tpdf` counts with the bench.
pub const COUNTER: [&str; 4] = [
    "--tool=cachegrind",
    "--cache-sim=no",
    "--cachegrind-out-file=/dev/null",
    "--trace-children=yes",
];

/// The variable the bench harness reads count mode from, which a checkout
/// whose harness has none does not mention.
const COUNT_VARIABLE: &str = "TINKER_BENCH_COUNT";

/// One operation's figure on one side: one warm iteration's instructions,
/// and the calls an iteration makes as that side's list gives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counted {
    /// A run of two iterations' count less a run of one's.
    pub instructions: u64,
    /// The `calls` the side's list gives the operation: at least one.
    pub calls: u64,
}

impl Counted {
    /// The warm iteration a run of one iteration and a run of two counted,
    /// over `calls`: refused when the second counted no more than the first,
    /// since that figure says nothing about the routine.
    pub fn warm(one: u64, two: u64, calls: u64) -> Result<Counted, String> {
        match two.checked_sub(one) {
            Some(instructions) if instructions > 0 => Ok(Counted {
                instructions,
                calls: calls.max(1),
            }),
            _ => Err(format!(
                "two iterations counted {two} instructions and one {one}, so a warm \
                 iteration counted none"
            )),
        }
    }
}

/// One operation's verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Judged {
    /// Counted on both sides: the head's against the base's.
    Both {
        /// The base's figure.
        base: Counted,
        /// The head's figure.
        head: Counted,
    },
    /// Counted at the head alone: the base's list does not hold it.
    New {
        /// The head's figure.
        head: Counted,
    },
}

/// The head's instructions a call over the base's, less one, in per cent:
/// negative for a fall.
fn change(base: Counted, head: Counted) -> f64 {
    let head_call = head.instructions as f64 / head.calls as f64;
    let base_call = base.instructions as f64 / base.calls as f64;
    (head_call / base_call - 1.0) * 100.0
}

impl Judged {
    /// The verdict on `head`, beside the base's figure where the base's list
    /// holds the operation.
    #[must_use]
    pub fn of(base: Option<Counted>, head: Counted) -> Judged {
        match base {
            Some(base) => Judged::Both { base, head },
            None => Judged::New { head },
        }
    }

    /// The head's instructions a call against the base's, in per cent,
    /// negative for a fall; `None` for an operation new at the head.
    #[must_use]
    pub fn percent(&self) -> Option<f64> {
        match *self {
            Judged::Both { base, head } => Some(change(base, head)),
            Judged::New { .. } => None,
        }
    }

    /// Whether the head's call counts more than [`RISE_PERCENT`] above the
    /// base's, decided in whole numbers so that a rise of exactly 1 % holds.
    /// A fall never fails, and neither does an operation new at the head.
    #[must_use]
    pub fn fails(&self) -> bool {
        match *self {
            Judged::Both { base, head } => {
                u128::from(head.instructions) * u128::from(base.calls) * 100
                    > u128::from(base.instructions)
                        * u128::from(head.calls)
                        * u128::from(100 + RISE_PERCENT)
            }
            Judged::New { .. } => false,
        }
    }

    /// The line a run prints for the operation `id`: the change, then the
    /// head's warm iteration against the base's, each with its calls where
    /// the two sides' lists give different ones.
    #[must_use]
    pub fn line(&self, id: &str) -> String {
        match *self {
            Judged::Both { base, head } if base.calls == head.calls => format!(
                "  {:+8.4} %  {} against {}  {id}",
                change(base, head),
                head.instructions,
                base.instructions
            ),
            Judged::Both { base, head } => format!(
                "  {:+8.4} %  {} in {} calls against {} in {} calls  {id}",
                change(base, head),
                head.instructions,
                head.calls,
                base.instructions,
                base.calls
            ),
            Judged::New { head } => {
                format!("  new at the head, not judged: {}  {id}", head.instructions)
            }
        }
    }
}

/// The figure after `I refs:` on a line of cachegrind's log, if the line is
/// that one: one the tool wrote for itself, `==<pid>==` ahead of it, whose
/// first word is `I` alone, so that no other event's line is read as it.
fn refs(line: &str) -> Option<&str> {
    let (pid, body) = line.trim_start().strip_prefix("==")?.split_once("==")?;
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let body = body.trim_start().strip_prefix('I')?;
    if !body.starts_with(char::is_whitespace) {
        return None;
    }
    Some(body.trim_start().strip_prefix("refs:")?.trim())
}

/// One process's instruction count, from the log cachegrind wrote for it:
/// the figure on its `I refs` line, `==<pid>== I   refs:      94,678,186`,
/// read past the pid and the grouping commas. The other lines carry figures
/// too, the pid, the version and the cache sizes the tool warns about, and
/// none of them is a count. A log with no count is a process that did not
/// reach its end, and one with two is not one process's.
pub fn instructions_in(log: &str) -> Result<u64, String> {
    let mut found = None;
    for line in log.lines() {
        let Some(figure) = refs(line) else {
            continue;
        };
        let digits: String = figure.chars().filter(|c| *c != ',').collect();
        let count = digits
            .parse::<u64>()
            .map_err(|_| format!("`{}` holds no count", line.trim()))?;
        if found.replace(count).is_some() {
            return Err("it holds two instruction counts, so it is not one process's".to_string());
        }
    }
    found.ok_or_else(|| {
        "it holds no instruction count, so its process did not reach its end".to_string()
    })
}

/// A run's count: every process's in the directory `logs`, a log each,
/// summed. A directory with no log, or a log with no count, is refused.
pub fn instructions(logs: &Path) -> Result<u64, String> {
    let entries =
        std::fs::read_dir(logs).map_err(|error| format!("{}: {error}", logs.display()))?;
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|e| e == "log"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("{}: no process wrote a log", logs.display()));
    }
    let mut total = 0u64;
    for file in &files {
        let text = std::fs::read_to_string(file)
            .map_err(|error| format!("{}: {error}", file.display()))?;
        let count = instructions_in(&text).map_err(|why| format!("{}: {why}", file.display()))?;
        total = total
            .checked_add(count)
            .ok_or_else(|| format!("{}: the counts overflow", logs.display()))?;
    }
    Ok(total)
}

/// The line count mode prints when it has run `id` `iterations` times.
#[must_use]
pub fn ran_line(id: &str, iterations: u64) -> String {
    format!("count-mode: RAN {id} {iterations}")
}

/// Whether a count-mode run ran what it was asked to: it exited 0 and its
/// standard output holds its [`ran_line`]. A run that did neither counted
/// something other than the operation, or nothing.
pub fn ran(succeeded: bool, stdout: &str, id: &str, iterations: u64) -> Result<(), String> {
    let line = ran_line(id, iterations);
    if !succeeded {
        return Err("the run did not exit 0".to_string());
    }
    if !stdout.lines().any(|printed| printed.trim_end() == line) {
        return Err(format!("the run did not print `{line}`"));
    }
    Ok(())
}

/// What a run was asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    /// The group, as `--group` names it.
    pub group: String,
    /// The part, `(k, n)` from `--part K/N`, or `None` for the whole group.
    pub part: Option<(usize, usize)>,
    /// The base's revision, or `None` for the head's parent.
    pub base: Option<String>,
    /// The head's revision, or `None` for `HEAD`.
    pub head: Option<String>,
}

/// `K/N`: the K-th of N parts, K from 1 to N.
pub fn part(text: &str) -> Result<(usize, usize), String> {
    let refused = || format!("`--part {text}` is no K/N with K from 1 to N");
    let (k, n) = text.split_once('/').ok_or_else(refused)?;
    let k: usize = k.trim().parse().map_err(|_| refused())?;
    let n: usize = n.trim().parse().map_err(|_| refused())?;
    if k == 0 || k > n {
        return Err(refused());
    }
    Ok((k, n))
}

/// The task's arguments: `--group NAME`, and `--part K/N`, `--base REV` and
/// `--head REV`, each at most once.
pub fn asked(args: &[String]) -> Result<Asked, String> {
    let mut group = None;
    let mut part_asked = None;
    let mut base = None;
    let mut head = None;
    let mut index = 0;
    while index < args.len() {
        let option = args[index].as_str();
        let value = || {
            args.get(index + 1)
                .cloned()
                .ok_or_else(|| format!("`{option}` needs a value"))
        };
        let once = |held: &Option<String>| match held {
            Some(_) => Err(format!("`{option}` is given twice")),
            None => value().map(Some),
        };
        match option {
            "--group" => group = once(&group)?,
            "--part" => {
                if part_asked.is_some() {
                    return Err("`--part` is given twice".to_string());
                }
                part_asked = Some(part(&value()?)?);
            }
            "--base" => base = once(&base)?,
            "--head" => head = once(&head)?,
            other => return Err(format!("unknown option `{other}`")),
        }
        index += 2;
    }
    let group = group.ok_or_else(|| {
        "bench-count needs --group NAME: it counts one group, or a part of one, at a time"
            .to_string()
    })?;
    Ok(Asked {
        group,
        part: part_asked,
        base,
        head,
    })
}

/// The operations the part `k` of `n` holds: every `n`-th of `operations`
/// from the `k`-th, or all of them with no part.
#[must_use]
pub fn in_part(operations: &[Listed], part: Option<(usize, usize)>) -> Vec<&Listed> {
    let (k, n) = part.unwrap_or((1, 1));
    operations
        .iter()
        .enumerate()
        .filter(|(index, _)| index % n == k - 1)
        .map(|(_, listed)| listed)
        .collect()
}

/// The base's entry for the operation `name`: `None` where the base has no
/// list for the group, or its list does not hold the operation, which is
/// then new at the head.
#[must_use]
pub fn at_base<'a>(base: Option<&'a Group>, name: &str) -> Option<&'a Listed> {
    base?.operations.iter().find(|listed| listed.name == name)
}

/// The executable cargo's JSON messages name for the bench target `group`:
/// its `compiler-artifact` message's, whose target is a bench of that name.
#[must_use]
pub fn bench_binary(messages: &str, group: &str) -> Option<String> {
    messages
        .lines()
        .filter_map(|line| json::parse(line).ok())
        .find_map(|message| {
            if message.get("reason")?.as_str()? != "compiler-artifact" {
                return None;
            }
            let target = message.get("target")?;
            let bench = target
                .get("kind")?
                .as_array()?
                .iter()
                .any(|kind| kind.as_str() == Some("bench"));
            if !bench || target.get("name")?.as_str()? != group {
                return None;
            }
            message.get("executable")?.as_str().map(str::to_string)
        })
}

/// `args`, as a command line reads them, for a message.
fn shown(args: &[OsString]) -> String {
    args.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The last lines of a child's standard error, for a message.
fn tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text.lines().collect();
    let kept = &lines[lines.len().saturating_sub(20)..];
    if kept.is_empty() {
        String::new()
    } else {
        format!("\n{}", kept.join("\n"))
    }
}

/// `git -C dir args`, its standard output trimmed, or why it failed.
fn git<S: AsRef<OsStr>>(dir: &Path, args: &[S]) -> Result<String, String> {
    let args: Vec<OsString> = args.iter().map(|arg| arg.as_ref().to_os_string()).collect();
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("git did not start: {error}"))?;
    if !out.status.success() {
        return Err(format!(
            "`git {}` failed with {}{}",
            shown(&args),
            out.status,
            tail(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The commit `rev` names in the repository at `root`, in full.
fn commit(root: &Path, rev: &str) -> Result<String, String> {
    let spec = format!("{rev}^{{commit}}");
    let named = git(root, &["rev-parse", "--verify", "--quiet", spec.as_str()])
        .map_err(|_| format!("`{rev}` names no commit in {}", root.display()))?;
    if named.is_empty() || !named.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("`{rev}` resolved to `{named}`, which is no commit"));
    }
    Ok(named)
}

/// The detached worktree of `commit` under [`WORK_DIR`], added when it is
/// not there yet. One that is there is used as it stands when it is that
/// commit's checkout, and refused otherwise, so a count is never of a
/// directory nobody can name the commit of.
fn checkout(root: &Path, commit: &str) -> Result<PathBuf, String> {
    let dir = root.join(WORK_DIR).join(commit);
    if dir.exists() {
        let refused = || {
            format!(
                "{} is not a checkout of {commit}; remove it with `git worktree remove \
                 --force {}` and run again",
                dir.display(),
                dir.display()
            )
        };
        let top = git(&dir, &["rev-parse", "--show-toplevel"]).map_err(|_| refused())?;
        let same = std::fs::canonicalize(&top)
            .ok()
            .zip(std::fs::canonicalize(&dir).ok())
            .is_some_and(|(top, dir)| top == dir);
        if !same || git(&dir, &["rev-parse", "HEAD"]).map_err(|_| refused())? != commit {
            return Err(refused());
        }
        return Ok(dir);
    }
    let parent = root.join(WORK_DIR);
    std::fs::create_dir_all(&parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    git(
        root,
        &[
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("--detach"),
            OsStr::new("--quiet"),
            dir.as_os_str(),
            OsStr::new(commit),
        ],
    )?;
    Ok(dir)
}

/// The groups the checkout at `dir` lists in its `baseline.json`, refused
/// when its bench harness has no count mode: such a bench binary would
/// ignore the variables and run every operation under criterion's sampler,
/// for hours under the counter, counting nothing it was asked for.
fn lists(dir: &Path, side: &str) -> Result<Vec<Group>, String> {
    let harness = dir.join(bench::BENCH_DIR).join("support").join("mod.rs");
    let counts = std::fs::read_to_string(&harness).is_ok_and(|text| text.contains(COUNT_VARIABLE));
    if !counts {
        return Err(format!(
            "the {side}'s bench harness, {}, has no count mode, so its operations cannot be \
             counted",
            harness.display()
        ));
    }
    let path = dir.join(bench::BASELINE_PATH);
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    bench::groups(&text, bench::BASELINE_PATH)
        .map_err(|error| format!("the {side}'s {}: {error}", bench::BASELINE_PATH))
}

/// The features the benchmark set is built with where a side's facade
/// manifest declares them (`docs/verification.md`, "The set is built with
/// `--features hyphenation`"): `hyphenation`, whose dictionaries the layout
/// group's hyphenated book needs. A side from before a feature was declared
/// builds without it, and its lists hold no operation that needs it.
pub const BENCH_FEATURES: &[&str] = &["hyphenation"];

/// The facade's manifest, from a checkout's root.
const FACADE_MANIFEST: &str = "crates/tinker-pdf/Cargo.toml";

/// Of [`BENCH_FEATURES`], each that `manifest`, a `Cargo.toml`, declares as a
/// key of its `[features]` table, in [`BENCH_FEATURES`]' order. A name met
/// anywhere else — a dependency's own feature list — declares nothing.
#[must_use]
pub fn declared_bench_features(manifest: &str) -> Vec<&'static str> {
    let mut declared: Vec<&str> = Vec::new();
    let mut in_features = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_features = line == "[features]";
        } else if in_features {
            if let Some((key, _)) = line.split_once('=') {
                declared.push(key.trim().trim_matches('"'));
            }
        }
    }
    BENCH_FEATURES
        .iter()
        .copied()
        .filter(|feature| declared.contains(feature))
        .collect()
}

/// The release `tpdf` and the group's release bench binary, built in the
/// checkout at `dir` into its own target directory, so the CLI operations
/// run the `tpdf` beside the bench, the bench with the [`BENCH_FEATURES`] the
/// checkout's facade declares: the bench binary's path.
fn build(dir: &Path, group: &str) -> Result<PathBuf, String> {
    let target = dir.join("target");
    let cargo = |args: &[&str]| {
        let mut command = Command::new(env!("CARGO"));
        command
            .args(args)
            .current_dir(dir)
            .env("CARGO_TARGET_DIR", &target)
            .stdin(Stdio::null());
        command
    };
    let tpdf = ["build", "--release", "-p", "tpdf"];
    let status = cargo(&tpdf)
        .status()
        .map_err(|error| format!("cargo did not start: {error}"))?;
    if !status.success() {
        return Err(format!(
            "`cargo {}` in {} failed with {status}",
            tpdf.join(" "),
            dir.display()
        ));
    }
    let manifest = std::fs::read_to_string(dir.join(FACADE_MANIFEST))
        .map_err(|error| format!("{}: {error}", dir.join(FACADE_MANIFEST).display()))?;
    let features = declared_bench_features(&manifest).join(",");
    let mut bench = vec![
        "bench",
        "-p",
        "tinker-pdf",
        "--bench",
        group,
        "--no-run",
        "--message-format=json-render-diagnostics",
    ];
    if !features.is_empty() {
        bench.extend(["--features", features.as_str()]);
    }
    let out = cargo(&bench)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("cargo did not start: {error}"))?;
    if !out.status.success() {
        return Err(format!(
            "`cargo {}` in {} failed with {}",
            bench.join(" "),
            dir.display(),
            out.status
        ));
    }
    bench_binary(&String::from_utf8_lossy(&out.stdout), group)
        .map(PathBuf::from)
        .ok_or_else(|| {
            format!(
                "`cargo {}` in {} named no bench binary `{group}`",
                bench.join(" "),
                dir.display()
            )
        })
}

/// What `valgrind --version` prints, or why the counter cannot run here.
fn counter() -> Result<String, String> {
    let out = Command::new("valgrind")
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            format!(
                "valgrind did not start ({error}): the counts run under its cachegrind, on Linux"
            )
        })?;
    if !out.status.success() {
        return Err(format!("`valgrind --version` failed with {}", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// One count: the bench binary `binary`, from the checkout at `dir`, run in
/// count mode on `id` for `iterations` under cachegrind, each process's log
/// in `logs`, emptied first, and their counts summed.
fn count(binary: &Path, dir: &Path, logs: &Path, id: &str, iterations: u64) -> Result<u64, String> {
    if logs.exists() {
        std::fs::remove_dir_all(logs).map_err(|error| format!("{}: {error}", logs.display()))?;
    }
    std::fs::create_dir_all(logs).map_err(|error| format!("{}: {error}", logs.display()))?;
    let mut log_file = OsString::from("--log-file=");
    log_file.push(logs.join("%p.log"));
    let out = Command::new("valgrind")
        .args(COUNTER)
        .arg(log_file)
        .arg(binary)
        .current_dir(dir)
        .env(COUNT_VARIABLE, id)
        .env("TINKER_BENCH_ITERATIONS", iterations.to_string())
        // A shared fixture directory keeps a file the first process wrote,
        // so the run of one would build an input the run of two skips.
        .env_remove("TINKER_BENCH_FIXTURES")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("valgrind did not start: {error}"))?;
    ran(
        out.status.success(),
        &String::from_utf8_lossy(&out.stdout),
        id,
        iterations,
    )
    .map_err(|why| {
        format!(
            "{id} at {iterations}: {why} ({}){}",
            out.status,
            tail(&out.stderr)
        )
    })?;
    instructions(logs).map_err(|why| format!("{id} at {iterations}: {why}"))
}

/// One side's figure for `id`: its runs of one iteration and of two, the
/// second less the first, over the `calls` the side's list gives it.
fn warm(binary: &Path, dir: &Path, logs: &Path, id: &str, calls: u64) -> Result<Counted, String> {
    let one = count(binary, dir, logs, id, 1)?;
    let two = count(binary, dir, logs, id, 2)?;
    Counted::warm(one, two, calls).map_err(|why| format!("{id}: {why}"))
}

/// `path` under `root`, as this repository names its paths.
fn under(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// `cargo xtask bench-count`: the group, or the part of it, asked for,
/// counted at the base and at the head and held to 1 % above the base.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let asked = asked(args)?;
    if bench::FROZEN.contains(&asked.group.as_str()) {
        return Err(format!(
            "`{}` has no count mode: its file is frozen and runs under criterion alone, and \
             its input and call are the text group's `text of 5 dense pages`, which count \
             mode runs",
            asked.group
        ));
    }
    let counter = counter()?;
    let head = commit(root, asked.head.as_deref().unwrap_or("HEAD"))?;
    let base = match &asked.base {
        Some(rev) => commit(root, rev)?,
        None => commit(root, &format!("{head}^"))?,
    };
    println!("bench-count: base {base}, head {head}, counted under {counter}'s cachegrind");

    let head_dir = checkout(root, &head)?;
    let base_dir = checkout(root, &base)?;
    let head_lists = lists(&head_dir, "head")?;
    let Some(group) = head_lists.iter().find(|group| group.name == asked.group) else {
        let names: Vec<&str> = head_lists.iter().map(|group| group.name.as_str()).collect();
        return Err(format!(
            "the head's lists hold no group `{}`; they hold {}",
            asked.group,
            names.join(", ")
        ));
    };
    let mine = in_part(&group.operations, asked.part);
    if mine.is_empty() {
        let (k, n) = asked.part.unwrap_or((1, 1));
        return Err(format!(
            "part {k} of {n} holds none of {}'s {} operations",
            group.name,
            group.operations.len()
        ));
    }
    let base_lists = lists(&base_dir, "base")?;
    let base_group = base_lists.iter().find(|listed| listed.name == group.name);

    println!(
        "bench-count: building the head in {}",
        under(root, &head_dir)
    );
    let head_binary = build(&head_dir, &group.name)?;
    let base_binary = match base_group {
        Some(_) => {
            println!(
                "bench-count: building the base in {}",
                under(root, &base_dir)
            );
            Some(build(&base_dir, &group.name)?)
        }
        None => {
            println!(
                "bench-count: the base lists no `{}` group, so each operation is new at the head",
                group.name
            );
            None
        }
    };

    let logs = root.join(WORK_DIR).join("logs").join(match asked.part {
        Some((k, n)) => format!("{}-{k}-of-{n}", group.name),
        None => group.name.clone(),
    });
    println!(
        "bench-count: {}, {} of its {} operations, each counted at one iteration and at two \
         on each side; each line is the change in a call's instructions, then the head's \
         warm iteration against the base's",
        group.name,
        mine.len(),
        group.operations.len()
    );
    let mut failing = Vec::new();
    let mut held = 0usize;
    let mut new = 0usize;
    for listed in &mine {
        let id = group.id(&listed.name);
        let base_counted = match (at_base(base_group, &listed.name), &base_binary) {
            (Some(at), Some(binary)) => {
                Some(warm(binary, &base_dir, &logs.join("base"), &id, at.calls)?)
            }
            _ => None,
        };
        let head_counted = warm(
            &head_binary,
            &head_dir,
            &logs.join("head"),
            &id,
            listed.calls,
        )?;
        let judged = Judged::of(base_counted, head_counted);
        println!("{}", judged.line(&id));
        match judged {
            Judged::New { .. } => new += 1,
            _ if judged.fails() => failing.push(judged.line(&id)),
            Judged::Both { .. } => held += 1,
        }
    }
    if let Some(base_group) = base_group {
        for listed in &base_group.operations {
            if !group.operations.iter().any(|at| at.name == listed.name) {
                println!(
                    "  not at the head, not counted: {}",
                    base_group.id(&listed.name)
                );
            }
        }
    }
    println!(
        "bench-count: {held} held within {RISE_PERCENT} % of the base, {} above it, {new} new \
         at the head",
        failing.len()
    );
    if failing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "more than {RISE_PERCENT} % above the base:\n{}",
            failing.join("\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The operation the recorded logs counted.
    const RECORDED: &str = "text/text of 5 dense pages, CLI";

    /// cachegrind's own logs of [`RECORDED`] counted at `iterations`, one
    /// and two, recorded once as every count runs it: valgrind 3.19.0,
    /// Debian bookworm's package, in a `rust:1-bookworm` container, running
    /// the text group's release bench binary with `TINKER_BENCH_COUNT` and
    /// `TINKER_BENCH_ITERATIONS` set, under [`COUNTER`] and
    /// `--log-file=<dir>/%p.log`. Each directory holds a log for the bench,
    /// one for the child criterion forks once a process to look for
    /// gnuplot, which that image does not hold, and one for each `tpdf` the
    /// operation starts, one a call, beside the bench's standard output.
    fn recorded(iterations: u64) -> PathBuf {
        crate::repo_root()
            .join("xtask/testdata/cachegrind/text-of-5-dense-pages-cli")
            .join(iterations.to_string())
    }

    /// The recorded warm iteration, as a run takes it from the two
    /// directories, over one call.
    fn recorded_warm() -> Counted {
        let one = instructions(&recorded(1)).expect("the run of one counts");
        let two = instructions(&recorded(2)).expect("the run of two counts");
        Counted::warm(one, two, 1).expect("a warm iteration")
    }

    /// The recorded figure with `instructions` in place of its own.
    fn at(instructions: u64) -> Counted {
        Counted {
            instructions,
            calls: 1,
        }
    }

    /// Each process's count is the figure on its `I refs` line, past the
    /// pid, the version and the cache sizes the same log carries, and a
    /// run's count is every process's summed: the bench's, the probe's and
    /// one `tpdf`'s a call.
    #[test]
    fn the_counts_are_read_from_the_counters_own_output() {
        let log = |iterations: u64, pid: &str| {
            std::fs::read_to_string(recorded(iterations).join(format!("{pid}.log")))
                .expect("the recorded log")
        };
        assert_eq!(instructions_in(&log(1, "1367")), Ok(42_134_350));
        assert_eq!(instructions_in(&log(1, "1368")), Ok(589_588));
        assert_eq!(instructions_in(&log(1, "1369")), Ok(94_678_186));
        assert_eq!(
            instructions(&recorded(1)),
            Ok(42_134_350 + 589_588 + 94_678_186)
        );
        assert_eq!(
            instructions(&recorded(2)),
            Ok(42_139_025 + 589_588 + 94_672_768 + 94_682_916)
        );
        // The run of two less the run of one: one more `tpdf`, and what the
        // bench spends starting it.
        assert_eq!(recorded_warm(), at(94_682_173));
        for iterations in [1, 2] {
            let stdout = std::fs::read_to_string(recorded(iterations).join("stdout"))
                .expect("the recorded standard output");
            assert_eq!(ran(true, &stdout, RECORDED, iterations), Ok(()));
        }
    }

    /// A rise of 1.1 % fails; the largest count within 1 % of the base
    /// holds and one instruction more does not, since the gate fails above
    /// 1 %, never at it.
    #[test]
    fn one_point_one_percent_above_base_fails() {
        let base = recorded_warm();
        let risen = Judged::of(Some(base), at(base.instructions * 1011 / 1000));
        assert!(risen.fails(), "{}", risen.line(RECORDED));
        assert!(risen.percent().is_some_and(|percent| percent > 1.0));
        let within = base.instructions * 101 / 100;
        assert!(!Judged::of(Some(base), at(within)).fails());
        assert!(Judged::of(Some(base), at(within + 1)).fails());
        assert!(!Judged::of(Some(base), base).fails());
    }

    /// Faster is never a failure, by however much: the gate is one-sided.
    #[test]
    fn a_fall_is_never_a_failure() {
        let base = recorded_warm();
        for head in [base.instructions - 1, base.instructions / 2, 1] {
            let judged = Judged::of(Some(base), at(head));
            assert!(!judged.fails(), "{}", judged.line(RECORDED));
            assert!(judged.percent().is_some_and(|percent| percent < 0.0));
        }
        assert_eq!(
            Judged::of(Some(base), at(base.instructions / 2)).line(RECORDED),
            format!(
                "  -50.0000 %  {} against {}  {RECORDED}",
                base.instructions / 2,
                base.instructions
            )
        );
    }

    /// An operation the base's list does not hold is counted at the head,
    /// printed as new and judged against nothing, so it never fails.
    #[test]
    fn an_operation_new_at_head_is_reported_and_not_failed() {
        let text = r#"{ "groups": [ { "group": "text", "operations": [
            { "name": "text of 5 dense pages", "unit": "pages", "per_iteration": 5, "calls": 1 }
        ] } ] }"#;
        let base_lists = bench::groups(text, "x").expect("the base's list parses");
        let base_group = base_lists.iter().find(|group| group.name == "text");
        assert!(at_base(base_group, "text of 5 dense pages").is_some());
        assert!(at_base(base_group, "text of 5 dense pages, CLI").is_none());
        assert!(at_base(None, "text of 5 dense pages").is_none());
        let head = recorded_warm();
        let judged = Judged::of(None, head);
        assert_eq!(judged, Judged::New { head });
        assert!(!judged.fails());
        assert_eq!(judged.percent(), None);
        assert_eq!(
            judged.line(RECORDED),
            format!("  new at the head, not judged: 94682173  {RECORDED}")
        );
    }

    /// Each side's iteration is taken over its own list's calls, so an
    /// operation run twice as often an iteration counts twice the
    /// instructions and has not risen.
    #[test]
    fn a_change_to_calls_moves_neither_side() {
        let base = recorded_warm();
        let twice = Counted {
            instructions: base.instructions * 2,
            calls: 2,
        };
        let judged = Judged::of(Some(base), twice);
        assert!(!judged.fails());
        assert_eq!(judged.percent(), Some(0.0));
        assert!(judged.line(RECORDED).contains(" in 2 calls against "));
        assert!(Judged::of(
            Some(base),
            Counted {
                instructions: base.instructions * 2 + base.instructions / 40,
                calls: 2,
            }
        )
        .fails());
    }

    /// A count that did not happen fails the run: a log cut off before its
    /// count, a directory with no log, a run that did not exit 0 or print
    /// its `RAN` line for the operation and the iterations asked, and a warm
    /// iteration that counted nothing.
    #[test]
    fn a_count_that_did_not_happen_fails_the_run() {
        let log = std::fs::read_to_string(recorded(1).join("1369.log")).expect("the log");
        let cut = &log[..log.find("I   refs").expect("the count")];
        assert!(instructions_in(cut).is_err());
        assert!(instructions_in(&format!("{log}{log}")).is_err());
        let no_logs = recorded(1).parent().expect("the recording").to_path_buf();
        assert!(instructions(&no_logs).is_err());
        let stdout = std::fs::read_to_string(recorded(1).join("stdout")).expect("stdout");
        assert!(ran(false, &stdout, RECORDED, 1).is_err());
        assert!(ran(true, &stdout, RECORDED, 2).is_err());
        assert!(ran(true, &stdout, "text/text of 5 dense pages", 1).is_err());
        assert!(ran(true, "", RECORDED, 1).is_err());
        assert!(Counted::warm(137_402_124, 137_402_124, 1).is_err());
        assert!(Counted::warm(137_402_124, 137_402_000, 1).is_err());
    }

    /// `--group` is required, each option is given once, and a part is K
    /// of N with K from 1 to N, holding every N-th operation from the K-th.
    #[test]
    fn a_part_holds_every_nth_operation_from_the_kth() {
        let args = |line: &str| line.split(' ').map(str::to_string).collect::<Vec<_>>();
        assert_eq!(
            asked(&args("--group open --part 2/6 --base abc")),
            Ok(Asked {
                group: "open".to_string(),
                part: Some((2, 6)),
                base: Some("abc".to_string()),
                head: None,
            })
        );
        assert!(asked(&args("--part 1/2")).is_err());
        assert!(asked(&args("--group open --group text")).is_err());
        assert!(asked(&args("--group open --part 1/2 --part 2/2")).is_err());
        assert!(asked(&args("--group")).is_err());
        assert!(asked(&args("--group open --jobs 4")).is_err());
        for refused in ["0/3", "4/3", "3", "a/3", "1/0"] {
            assert!(part(refused).is_err(), "{refused}");
        }
        let text = r#"{ "groups": [ { "group": "open", "operations": [
            { "name": "a", "unit": "none", "per_iteration": 1, "calls": 1 },
            { "name": "b", "unit": "none", "per_iteration": 1, "calls": 1 },
            { "name": "c", "unit": "none", "per_iteration": 1, "calls": 1 },
            { "name": "d", "unit": "none", "per_iteration": 1, "calls": 1 },
            { "name": "e", "unit": "none", "per_iteration": 1, "calls": 1 }
        ] } ] }"#;
        let lists = bench::groups(text, "x").expect("the list parses");
        let names = |part| {
            in_part(&lists[0].operations, part)
                .iter()
                .map(|listed| listed.name.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(None), ["a", "b", "c", "d", "e"]);
        assert_eq!(names(Some((2, 2))), ["b", "d"]);
        assert_eq!(names(Some((1, 3))), ["a", "d"]);
        assert!(names(Some((6, 6))).is_empty());
    }

    /// The bench binary is the executable of the `compiler-artifact` message
    /// whose target is a bench of the group's name, in cargo's own messages:
    /// these three from a `--message-format=json-render-diagnostics` build
    /// of the text group, the library's artifact, the bench's and the end.
    #[test]
    fn the_bench_binary_is_read_from_cargos_own_messages() {
        let messages = concat!(
            r#"{"reason":"compiler-artifact","package_id":"path+file:///src/crates/tinker-pdf#0.0.1","manifest_path":"/src/crates/tinker-pdf/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"tinker_pdf","src_path":"/src/crates/tinker-pdf/src/lib.rs","edition":"2021","doc":true,"doctest":true,"test":true},"profile":{"opt_level":"3","debuginfo":0,"debug_assertions":false,"overflow_checks":false,"test":false},"features":["cmap-predefined","default"],"filenames":["/target/release/deps/libtinker_pdf-34181160988c3791.rlib","/target/release/deps/libtinker_pdf-34181160988c3791.rmeta"],"executable":null,"fresh":true}"#,
            "\n",
            r#"{"reason":"compiler-artifact","package_id":"path+file:///src/crates/tinker-pdf#0.0.1","manifest_path":"/src/crates/tinker-pdf/Cargo.toml","target":{"kind":["bench"],"crate_types":["bin"],"name":"text","src_path":"/src/crates/tinker-pdf/benches/text.rs","edition":"2021","doc":false,"doctest":false,"test":false},"profile":{"opt_level":"3","debuginfo":0,"debug_assertions":false,"overflow_checks":false,"test":true},"features":["cmap-predefined","default"],"filenames":["/target/release/deps/text-2bc186b284ba082d"],"executable":"/target/release/deps/text-2bc186b284ba082d","fresh":true}"#,
            "\n",
            r#"{"reason":"build-finished","success":true}"#,
            "\n",
        );
        assert_eq!(
            bench_binary(messages, "text").as_deref(),
            Some("/target/release/deps/text-2bc186b284ba082d")
        );
        assert_eq!(bench_binary(messages, "tinker_pdf"), None);
        assert_eq!(bench_binary(messages, "render"), None);
    }

    /// A side builds the set with each of [`BENCH_FEATURES`] its facade's
    /// manifest declares under `[features]`, a quoted key as a bare one, and
    /// without one it lacks: a name met outside the table, in a dependency's
    /// own feature list, declares nothing. The committed manifest declares
    /// every one of them.
    #[test]
    fn a_side_builds_with_the_bench_features_its_manifest_declares() {
        let declares = concat!(
            "[package]\nname = \"tinker-pdf\"\n\n",
            "[features]\ndefault = [\"cmap-predefined\"]\n",
            "hyphenation = [\"tinker-pdf-layout/hyphenation\"]\n\n",
            "[dependencies]\nx = \"1\"\n",
        );
        assert_eq!(declared_bench_features(declares), ["hyphenation"]);
        let quoted = "[features]\n\"hyphenation\" = []\n";
        assert_eq!(declared_bench_features(quoted), ["hyphenation"]);
        let before = concat!(
            "[features]\ndefault = []\nbundled-fonts = [\"tinker-pdf-font/bundled-fonts\"]\n\n",
            "[dependencies]\n",
            "tinker-pdf-layout = { path = \"../tinker-pdf-layout\", features = [\"hyphenation\"] }\n",
            "hyphenation = \"1\"\n",
        );
        assert!(declared_bench_features(before).is_empty());
        assert!(declared_bench_features("").is_empty());
        let committed = std::fs::read_to_string(crate::repo_root().join(FACADE_MANIFEST))
            .expect("the facade's manifest");
        assert_eq!(declared_bench_features(&committed), BENCH_FEATURES);
    }
}
