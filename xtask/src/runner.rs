//! One child process per file, with a timeout.
//!
//! This is the whole reason a corpus runner is a program rather than a shell
//! loop. A corpus is thousands of files chosen for being difficult; some of
//! them will find a panic, and sooner or later one will find a loop that does
//! not terminate. In one process the first takes the run down and the second
//! takes it down slower, and either way the report that comes out describes
//! the files before the bad one and says nothing about the rest — which reads
//! exactly like a corpus where the rest do not exist.
//!
//! So: a process per file, killed after a timeout, and **both** states are
//! results rather than absences. A file that aborts is `Crashed`; a file that
//! runs out of time is `TimedOut` or `Stalled`; the report carries them and
//! the run continues.
//!
//! ## How a hang is told from a slow file
//!
//! By whether the child was still saying anything. Rust's stdout is a
//! `LineWriter`, so every line the child prints reaches the capture file when
//! it is printed rather than when the process ends — which means the capture
//! file's *length* is a progress signal available to the runner for free,
//! without a pipe, a second thread or a protocol.
//!
//! A child killed at the timeout having written something recently was making
//! progress and is `TimedOut`: a slow file. One that had written nothing for
//! half its budget is `Stalled`, and that is the state a non-terminating loop
//! produces. The distinction is not free of judgement and the limit is worth
//! stating plainly: a single unit of work longer than the stall window — one
//! enormous page — is silent for the same reason a hang is, and reads as
//! `Stalled`. What the runner can say honestly is "made no observable progress
//! for half its budget", and `at` names the phase it was in when it stopped.
//!
//! This was written because a real one got through. `pdfjs/test/pdfs/
//! bug1980958.pdf` is 219 bytes and rendered in under two seconds, and a
//! rewrite of it did not terminate; the corpus run recorded a timeout, which
//! is what it records for a 900-page scan, and nobody looked.
//!
//! ## How a crash is told from a failure
//!
//! By a sentinel, not by an exit code. The child writes `done` as the last
//! line of a complete record, so:
//!
//! - record ends in `done` — the child finished, and the record says what it
//!   found, including that a file would not open;
//! - no `done`, and we killed it — `TimedOut` or `Stalled`;
//! - no `done`, and it exited on its own — `Crashed`, whatever the status.
//!
//! An exit code alone cannot do this. A panic that unwinds to a `main`
//! returning `Ok` exits 0, and `abort` on Windows exits with a status that a
//! naive reader sees as a large positive number rather than as a signal.
//! Neither is distinguishable from success by status; both are obvious by the
//! missing sentinel.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Distinguishes one child's captured output from another's within a process.
static NEXT_CAPTURE: AtomicU64 = AtomicU64::new(0);

/// The program the runner spawns, and the arguments before the file name.
#[derive(Clone, Debug)]
pub struct Child {
    /// The executable.
    pub program: PathBuf,
    /// Everything before the file path, which is always last.
    pub args: Vec<String>,
}

/// What happened to one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Opened, and every page returned a bitmap.
    ///
    /// Ruling 2's definition and not a stricter one: a page carrying a JBIG2
    /// placeholder returned a bitmap, so it passed. Degradation is the other
    /// axis — see `warnings` — and counting it here would make every honest
    /// degradation a regression and push the ratchet the wrong way.
    Passed,
    /// The child finished and reported that the file did not open, or that a
    /// page it was asked for produced nothing.
    Failed(String),
    /// The child died without finishing its record.
    Crashed(String),
    /// The child was still running when the timeout expired, and was killed
    /// while it was still writing. A slow file.
    ///
    /// `at` is the last phase the child announced, or an empty string from a
    /// child too old to announce one.
    TimedOut { at: String },
    /// The child was killed at the timeout having written nothing for half of
    /// it. A hang, or one unit of work longer than that window.
    Stalled { at: String },
}

impl Outcome {
    /// The single word the report and the summary use.
    pub fn label(&self) -> &'static str {
        match self {
            Outcome::Passed => "passed",
            Outcome::Failed(_) => "failed",
            Outcome::Crashed(_) => "crashed",
            Outcome::TimedOut { .. } => "timed_out",
            Outcome::Stalled { .. } => "stalled",
        }
    }
}

/// One file's result.
#[derive(Clone, Debug)]
pub struct FileResult {
    /// The path as the report names it: relative to the corpus's file root,
    /// with forward slashes, so a report made on Windows and one made on
    /// Linux compare.
    pub path: String,
    /// What happened.
    pub outcome: Outcome,
    /// Pages the document claimed.
    pub pages: u32,
    /// Pages that returned a bitmap.
    pub rendered: u32,
    /// Warning labels and how many times each occurred.
    pub warnings: BTreeMap<String, usize>,
    /// What the file's objects say it needs: `jbig2`, `jpx`, `mesh-shading`.
    pub capabilities: BTreeSet<String>,
    /// How long the child took, in milliseconds, as the runner measured it.
    pub millis: u64,
    /// What the strict validator said about a **rewrite** of this file, or
    /// why the file was not eligible for one (ruling 13).
    pub strict: Strict,
    /// What the metamorphic relations said about the first page, by name
    /// (roadmap step 7): `rotate`, `crop`, `dpi`.
    pub metamorphic: BTreeMap<String, MetaVerdict>,
    /// Whether the child that produced this record was built with the twelve
    /// bundled faces.
    ///
    /// The child's own `cfg`, not a flag: a run measured against faces is a
    /// different measurement from one without them, and this is what stops the
    /// two being recorded in each other's slot.
    pub bundled_faces: bool,
    /// What this document costs to work on, as properties of the document.
    ///
    /// Reported so that the gate deciding which files the relations are asked
    /// of can be a function of the corpus rather than of the machine. Zero
    /// from a child that did not say.
    pub cost: Cost,
    /// The child's peak resident set in bytes, or `None` where it did not say.
    ///
    /// Measured **by the child, of itself**, and `None` carries that fact
    /// rather than hiding it. Two things produce a `None` and both must stay
    /// visible: a platform whose `tpdf` cannot read a high-water mark, which
    /// makes the run incomplete; and a child that died before it could print
    /// the line, which is already a crash. A zero would merge them into a
    /// measurement, and a measurement of zero is a ceiling nothing can exceed.
    pub peak: Option<u64>,
    /// `/Producer` as the document states it, or `None` from a child that did
    /// not say.
    ///
    /// **Not defaulted to an empty string**, and the distinction is the one
    /// the whole field exists for: `Some("(none stated)")` is a document that
    /// declares no producer, which is a population worth counting, and `None`
    /// is a record that predates this key or a file that never opened. A
    /// report that merged them would attribute every crash to the same
    /// imaginary producer.
    pub producer: Option<String>,
    /// What the structure tree walk found, or `None` where the document has
    /// no `/StructTreeRoot` this engine could read.
    ///
    /// `None` and "no tree" are the same answer here and deliberately so: the
    /// child says `tagged tree no` in that case, and a record that mentions
    /// the key not at all is refused earlier, by the version check.
    pub tagged: Option<Tagged>,
}

/// What one document's structure tree yielded (ISO 32000-1 14.7).
///
/// Counts rather than rates, for the reason [`Cost`] holds counts: a rate
/// computed per file and averaged is not the rate over the corpus, and the
/// bar in `corpus/ratchet.json` is over the corpus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tagged {
    /// Structure elements reached by the `/K` walk.
    pub elements: u64,
    /// Marked-content kids — `/MCID` integers and `/MCR` dictionaries.
    pub content: u64,
    /// `/OBJR` kids: annotations and XObjects a structure element claims.
    pub objects: u64,
    /// Characters a structure element claimed, over the pages the child
    /// rendered.
    pub matched: u64,
    /// Characters carrying an `/MCID` no element on their page claimed.
    pub orphans: u64,
    /// Characters carrying no `/MCID` at all, inside a document that has a
    /// structure tree.
    pub unmarked: u64,
}

/// A document's size, in the three dimensions that bound work on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cost {
    /// The file's length.
    pub bytes: u64,
    /// How many object numbers the cross-reference table has entries for.
    pub objects: u64,
    /// Device pixels in the first page at the run's resolution.
    pub pixels: u64,
}

/// One metamorphic relation's verdict on one file.
///
/// Three states and not two, for `Strict`'s reason: a relation that could not
/// be asked — a page too large to render twice, a rewrite that would not
/// reopen — must not be counted as one that held, or the rate flatters itself
/// by declining the hard files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetaVerdict {
    /// The relation held, and this is how far it was from not holding —
    /// `12 of 501832` pixels, as the child measured it.
    ///
    /// A child that predates the measurement says only `held` and the string
    /// is empty, which is why this is not an `Option` of a pair: an older
    /// record is not a newer record with a field missing, it is a record that
    /// did not measure. Nothing reads the number back into arithmetic; it is
    /// there so `report.json` carries the distribution a budget is re-sited
    /// from, rather than only its tail.
    Held(String),
    /// It did not, and this is what the child measured.
    Broke(String),
    /// It was not asked, and this is why.
    Skipped(String),
}

impl MetaVerdict {
    /// Whether the relation was asked at all.
    pub fn compared(&self) -> bool {
        !matches!(self, MetaVerdict::Skipped(_))
    }

    /// Whether it held.
    pub fn held(&self) -> bool {
        matches!(self, MetaVerdict::Held(_))
    }

    /// The word the report writes.
    pub fn label(&self) -> &'static str {
        match self {
            MetaVerdict::Held(_) => "held",
            MetaVerdict::Broke(_) => "broke",
            MetaVerdict::Skipped(_) => "skipped",
        }
    }

    /// What the child said, where it said anything.
    pub fn detail(&self) -> &str {
        match self {
            MetaVerdict::Held(detail)
            | MetaVerdict::Broke(detail)
            | MetaVerdict::Skipped(detail) => detail,
        }
    }
}

/// The strict pass's verdict on one file.
///
/// `Ineligible` is not a failure and not a pass: the ratchet counts eligible
/// files and clean ones, so a corpus of files this engine cannot read cleanly
/// cannot flatter the rate by being counted as either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Strict {
    /// The child did not run the pass, and said why.
    Ineligible(String),
    /// It ran, and the rewrite carried this many defects.
    Checked {
        /// [`Tier::Structure`] defects — the writer's own.
        structure: u64,
        /// [`Tier::Semantics`] defects — inherited from the source.
        semantics: u64,
        /// How many defects of each kind, by label.
        kinds: BTreeMap<String, usize>,
    },
}

impl Strict {
    /// Whether the pass ran at all.
    pub fn eligible(&self) -> bool {
        matches!(self, Strict::Checked { .. })
    }

    /// Whether it ran and found nothing the writer owns.
    pub fn clean(&self) -> bool {
        matches!(self, Strict::Checked { structure: 0, .. })
    }
}

impl FileResult {
    /// Whether this file rendered with anything reported.
    ///
    /// The second axis. A file can pass and still be here, which is the point:
    /// "it came back" and "it came back right" are different questions and a
    /// single number cannot answer both.
    pub fn degraded(&self) -> bool {
        !self.warnings.is_empty()
    }
}

/// Runs one file in its own process and waits, with a limit.
/// `extra` is appended after the child's own arguments and before the file —
/// today only `--password P`, from `corpus/passwords.tsv`. It is a slice
/// rather than an `Option<&str>` so the next thing a single file needs of the
/// child does not change this signature again.
pub fn run_one(
    child: &Child,
    file: &Path,
    relative: &str,
    timeout: Duration,
    extra: &[String],
) -> FileResult {
    let started = Instant::now();
    let empty = |outcome: Outcome, millis: u64| FileResult {
        path: relative.to_string(),
        outcome,
        pages: 0,
        rendered: 0,
        warnings: BTreeMap::new(),
        capabilities: BTreeSet::new(),
        millis,
        strict: Strict::Ineligible("the child produced no record".to_string()),
        // A child that wrote no record asked no relation, which is not the
        // same as a relation that held: an empty map counts as neither
        // compared nor held.
        metamorphic: BTreeMap::new(),
        bundled_faces: false,
        cost: Cost::default(),
        // A child that wrote no record measured no peak. `None` rather than a
        // zero for the reason the field carries: this file is already counted
        // as a crash, and a zero here would be a measurement.
        peak: None,
        // A child that wrote no record did not look at a structure tree, and
        // `None` is the same answer as "there was none". They aggregate the
        // same way, and the file is already counted as failed.
        tagged: None,
        // And it never read an `/Info` dictionary either.
        producer: None,
    };

    // Both streams go to temporary files rather than to pipes. A pipe whose
    // reader is the same thread that waits for the child deadlocks the moment
    // the child writes more than the pipe buffer holds, and `wait_with_output`
    // — which reads and waits together — offers no way to stop waiting, which
    // is precisely what a timeout has to do.
    //
    // Named by a counter as well as by the process and the path. The path
    // alone is not unique enough: two runs inside one process — which is what
    // the isolation tests are — would otherwise share a capture file for any
    // file of the same name, and each would read the other's record. That
    // produced exactly the failure this whole module exists to prevent, a
    // healthy file reported as a crash, from a test harness rather than from
    // the engine.
    let serial = NEXT_CAPTURE.fetch_add(1, Ordering::Relaxed);
    let stem = std::env::temp_dir().join(format!(
        "tinker-corpus-{}-{serial}-{:x}",
        std::process::id(),
        hash(relative)
    ));
    let out_path = stem.with_extension("out");
    let err_path = stem.with_extension("err");
    let (Ok(out_file), Ok(err_file)) = (
        std::fs::File::create(&out_path),
        std::fs::File::create(&err_path),
    ) else {
        return empty(
            Outcome::Crashed(format!(
                "could not create the child's output files under {}",
                std::env::temp_dir().display()
            )),
            0,
        );
    };

    let spawned = Command::new(&child.program)
        .args(&child.args)
        .args(extra)
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file))
        .spawn();

    let mut process = match spawned {
        Ok(process) => process,
        Err(error) => {
            let _ = std::fs::remove_file(&out_path);
            let _ = std::fs::remove_file(&err_path);
            return empty(
                Outcome::Crashed(format!(
                    "{} could not be run: {error}",
                    child.program.display()
                )),
                0,
            );
        }
    };

    // How long the child may write nothing before the runner stops calling it
    // slow and starts calling it stalled. Half the budget: a proportion rather
    // than a constant, because it has to mean the same thing at `--timeout 5`
    // and `--timeout 60`, and because a floor large enough to be safe at one
    // end is larger than the whole budget at the other. `--timeout 0` is
    // refused on the command line, so this is never zero.
    let stall_window = timeout / 2;

    let mut killed = false;
    let mut said = 0u64;
    let mut last_spoke = started;
    // The capture file's length is checked on its own clock rather than every
    // poll: four thousand children polled every two milliseconds would be a
    // great many `stat` calls to answer a question that changes slowly.
    let mut next_look = started;
    loop {
        match process.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(error) => {
                let _ = process.kill();
                let _ = process.wait();
                killed = true;
                let _ = error;
                break;
            }
        }
        let now = Instant::now();
        if now >= next_look {
            next_look = now + Duration::from_millis(100);
            let written = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
            if written != said {
                said = written;
                last_spoke = now;
            }
        }
        if started.elapsed() >= timeout {
            let _ = process.kill();
            let _ = process.wait();
            killed = true;
            break;
        }
        // Short enough that a fast file is not padded by the poll interval,
        // long enough that four thousand of them do not spin a core.
        std::thread::sleep(Duration::from_millis(2));
    }
    let silent_for = last_spoke.elapsed();

    let millis = started.elapsed().as_millis() as u64;
    let stdout = read_and_remove(&out_path);
    let stderr = read_and_remove(&err_path);

    let mut result = match parse_record(&stdout) {
        Some(mut result) => {
            result.path = relative.to_string();
            result.millis = millis;
            result
        }
        None => {
            let detail = last_meaningful_line(&stderr)
                .unwrap_or_else(|| "the child wrote no complete record".to_string());
            let mut empty = empty(Outcome::Crashed(detail), millis);
            // **The producer survives an incomplete record, and nothing else
            // does.** Everything else in a record is a *result* -- what the
            // relations said, what the strict pass found -- and a result from
            // a child that did not finish is not a result. `/Producer` is not:
            // it is a property of the document, printed before any of the work
            // that then failed to finish.
            //
            // Keeping it is the difference between a producer table that
            // attributes failures and one that cannot. Measured on the
            // production corpus's first run: eighteen files did not pass and
            // all eighteen read `(unread: the file did not open)`, because
            // fourteen of them were killed at the timeout with the producer
            // line already in the capture file and thrown away here.
            empty.producer = producer_line(&stdout);
            empty
        }
    };

    // A killed child may still have flushed a complete record in the instant
    // before it died; it is a timeout all the same, because the run had
    // already decided not to wait for it and counting it as a pass would make
    // the timeout depend on scheduling luck.
    if killed {
        let at = last_phase(&stdout);
        result.outcome = if silent_for >= stall_window {
            Outcome::Stalled { at }
        } else {
            Outcome::TimedOut { at }
        };
    }
    result
}

fn read_and_remove(path: &Path) -> String {
    let mut text = String::new();
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut bytes = Vec::new();
        let _ = file.read_to_end(&mut bytes);
        text = String::from_utf8_lossy(&bytes).into_owned();
    }
    let _ = std::fs::remove_file(path);
    text
}

/// The last phase the child announced, or an empty string.
///
/// `phase` is an ordinary record key, so a child that does not write one is
/// not an error — it is a child that cannot say where it stopped, and the
/// stalled/slow verdict does not depend on it. The `page k/n` lines a render
/// writes count too, because "stopped at page 340 of 900" is the most useful
/// answer this can give.
pub fn last_phase(text: &str) -> String {
    text.lines()
        .rev()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("phase ")
                .or_else(|| line.strip_prefix("page ").map(|_| line))
                .map(|rest| rest.trim().chars().take(60).collect::<String>())
        })
        .unwrap_or_default()
}

/// The `producer` line out of a record that may be incomplete.
///
/// Scanned rather than parsed, because the record this reads is by definition
/// one `parse_record` refused: there is no `done`, the last line may be half
/// written, and every other key in it is untrustworthy for that reason.
fn producer_line(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("producer "))
        .map(|rest| rest.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn last_meaningful_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(|line| line.chars().take(200).collect())
}

/// A cheap, stable name for a temporary file. Not a checksum — it only has to
/// keep two files being probed at once from colliding.
fn hash(text: &str) -> u64 {
    let mut value = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.as_bytes() {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x1000_0000_01b3);
    }
    value
}

/// The record format's version. A record announcing anything else is refused
/// rather than half-read.
///
/// Version 4 adds the `signature` capability: reading a signature produces no
/// warning when it succeeds, so a count is the only thing that can say whether
/// the reader is still finding them all.
///
/// Version 6 adds `peak`, the child's own peak resident set. An unknown key is
/// ordinarily ignored, so a new key alone would not need a bump — but this one
/// is *required*: a corpus where no child reports a peak makes the run
/// incomplete. Without the bump, running against a version-5 binary would look
/// like a platform that cannot measure memory rather than like a `tpdf` nobody
/// rebuilt, and `corpus.rs`'s handshake exists precisely to name the second.
pub const PROBE_VERSION: u32 = 6;

/// Reads a child's record, or `None` if it is not complete.
///
/// Completeness is `done` on a line of its own. Everything else is best
/// effort — an unknown key is ignored, so a newer child adding a key does not
/// make an older runner call every file a crash — but the sentinel is not,
/// because the sentinel is the only thing separating "this file failed" from
/// "this process died", and those must never merge.
pub fn parse_record(text: &str) -> Option<FileResult> {
    let mut version = None;
    let mut opened = None;
    let mut failure = None;
    let mut pages = 0u32;
    let mut rendered = 0u32;
    let mut millis = 0u64;
    let mut complete = false;
    let mut warnings = BTreeMap::new();
    let mut capabilities = BTreeSet::new();
    let mut why_not: Option<String> = None;
    let mut eligible = None;
    let mut structure = 0u64;
    let mut semantics = 0u64;
    let mut defects: BTreeMap<String, usize> = BTreeMap::new();
    let mut metamorphic: BTreeMap<String, MetaVerdict> = BTreeMap::new();
    let mut cost = Cost::default();
    let mut bundled_faces = false;
    let mut peak: Option<u64> = None;
    let mut tagged: Option<Tagged> = None;
    let mut producer: Option<String> = None;

    for line in text.lines() {
        let line = line.trim_end_matches(['\r', '\n']);
        if line == "done" {
            complete = true;
            continue;
        }
        let (key, rest) = match line.split_once(' ') {
            Some(pair) => pair,
            None => continue,
        };
        match key {
            "probe" => version = rest.trim().parse::<u32>().ok(),
            "pages" => pages = rest.trim().parse().unwrap_or(0),
            // Taken whole rather than split: a producer string contains
            // anything at all -- spaces, tabs the child has already folded,
            // and in this corpus a pipe character where one tool recorded two.
            "producer" => producer = Some(rest.trim().to_string()),
            "rendered" => rendered = rest.trim().parse().unwrap_or(0),
            "ms" => millis = rest.trim().parse().unwrap_or(0),
            "opened" => {
                let (verdict, reason) = rest.split_once(' ').unwrap_or((rest.trim(), ""));
                opened = Some(verdict.trim() == "yes");
                if verdict.trim() != "yes" {
                    failure = Some(if reason.trim().is_empty() {
                        "the file did not open".to_string()
                    } else {
                        reason.trim().to_string()
                    });
                }
            }
            "build" => {
                if rest.trim() == "bundled-fonts" {
                    bundled_faces = true;
                }
            }
            // Unparseable stays `None` rather than becoming a zero: the whole
            // value of the field is that "not measured" and "measured as
            // nothing" are different answers.
            "peak" => peak = rest.trim().parse::<u64>().ok(),
            "cost" => {
                let mut rest = rest.split_whitespace();
                while let (Some(field), Some(value)) = (rest.next(), rest.next()) {
                    let value = value.parse().unwrap_or(0);
                    match field {
                        "bytes" => cost.bytes = value,
                        "objects" => cost.objects = value,
                        "pixels" => cost.pixels = value,
                        _ => {}
                    }
                }
            }
            "tagged" => {
                let (what, rest) = rest.split_once(' ').unwrap_or((rest.trim(), ""));
                match what.trim() {
                    // `tree no` leaves `tagged` at `None`, which is what a
                    // document with no structure tree means.
                    "tree" => {
                        let mut fields = rest.split_whitespace();
                        if fields.next() == Some("yes") {
                            let slot = tagged.get_or_insert_with(Tagged::default);
                            while let (Some(field), Some(value)) = (fields.next(), fields.next()) {
                                let value = value.parse().unwrap_or(0);
                                match field {
                                    "elements" => slot.elements = value,
                                    "content" => slot.content = value,
                                    "objects" => slot.objects = value,
                                    _ => {}
                                }
                            }
                        }
                    }
                    // Only ever printed by a child that already printed
                    // `tree yes`, so a `chars` line with no tree before it is
                    // a child bug and is dropped rather than inventing a tree.
                    "chars" => {
                        if let Some(slot) = tagged.as_mut() {
                            let mut fields = rest.split_whitespace();
                            while let (Some(field), Some(value)) = (fields.next(), fields.next()) {
                                let value = value.parse().unwrap_or(0);
                                match field {
                                    "matched" => slot.matched = value,
                                    "orphans" => slot.orphans = value,
                                    "unmarked" => slot.unmarked = value,
                                    _ => {}
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            "cap" => {
                capabilities.insert(rest.trim().to_string());
            }
            "warn" => {
                let (label, count) = rest.rsplit_once(' ').unwrap_or((rest.trim(), "1"));
                let count: usize = count.trim().parse().unwrap_or(1);
                *warnings.entry(label.trim().to_string()).or_default() += count;
            }
            "meta" => {
                let (name, rest) = rest.split_once(' ').unwrap_or((rest.trim(), ""));
                let (verdict, detail) = rest.split_once(' ').unwrap_or((rest.trim(), ""));
                let detail = detail.trim().to_string();
                let verdict = match verdict.trim() {
                    "held" => MetaVerdict::Held(detail),
                    "broke" => MetaVerdict::Broke(detail),
                    // An unknown word is a skip with the word in it rather than
                    // a hold: a runner that read a verdict it did not know as
                    // "the relation held" would count a newer child's failures
                    // as successes.
                    other => MetaVerdict::Skipped(if detail.is_empty() {
                        other.to_string()
                    } else {
                        detail
                    }),
                };
                metamorphic.insert(name.trim().to_string(), verdict);
            }
            "strict" => {
                let (what, rest) = rest.split_once(' ').unwrap_or((rest.trim(), ""));
                match what.trim() {
                    "eligible" => eligible = Some(String::new()),
                    "ineligible" => {
                        let reason = rest.trim();
                        eligible = None;
                        why_not = Some(if reason.is_empty() {
                            "the child did not say why".to_string()
                        } else {
                            reason.to_string()
                        });
                    }
                    "structure" => structure = rest.trim().parse().unwrap_or(0),
                    "semantics" => semantics = rest.trim().parse().unwrap_or(0),
                    "kind" => {
                        let (label, count) = rest.rsplit_once(' ').unwrap_or((rest.trim(), "1"));
                        let count: usize = count.trim().parse().unwrap_or(1);
                        *defects.entry(label.trim().to_string()).or_default() += count;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    if !complete {
        return None;
    }
    // A complete record in a format this runner does not know is worse than
    // no record: its fields may mean something else entirely.
    if version != Some(PROBE_VERSION) {
        return None;
    }

    let strict = match (eligible.is_some(), why_not) {
        (true, _) => Strict::Checked {
            structure,
            semantics,
            kinds: defects,
        },
        // A record that says nothing about the pass at all is one from a child
        // that skipped it, which is a defect in the child rather than a fact
        // about the file — so it is not eligible and says so.
        (false, Some(reason)) => Strict::Ineligible(reason),
        (false, None) => Strict::Ineligible("the record does not mention it".to_string()),
    };

    let outcome = match opened {
        Some(true) if rendered >= pages => Outcome::Passed,
        Some(true) => Outcome::Failed(format!("{rendered} of {pages} pages rendered")),
        Some(false) => Outcome::Failed(failure.unwrap_or_else(|| "the file did not open".into())),
        // A complete record that never said whether the file opened is a
        // child bug, and reading it as a pass would hide it.
        None => Outcome::Failed("the record does not say whether the file opened".to_string()),
    };

    Some(FileResult {
        path: String::new(),
        outcome,
        pages,
        rendered,
        warnings,
        capabilities,
        millis,
        strict,
        metamorphic,
        bundled_faces,
        cost,
        peak,
        producer,
        tagged,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "probe 6\nfile x.pdf\nopened yes\nladder Trust\npages 3\n\
                        cap jbig2\nrendered 3\nstrict eligible\nstrict structure 0\n\
                        strict semantics 2\nstrict kind annot-rect-unordered 2\n\
                        warn render:UnreadableFont 2\nms 40\ndone\n";

    /// A record built line by line, so a test's expectations are readable as
    /// the child's own output rather than as one escaped string.
    fn record(lines: &[&str]) -> String {
        let mut out = lines.join("\n");
        out.push('\n');
        out
    }

    /// The structure counts arrive on two lines because they are measured at
    /// two different times -- the tree once per document, the join once per
    /// page -- and a child that printed the first and died before the second
    /// must not have its zero read as a measurement.
    #[test]
    fn the_structure_counts_read_off_both_lines() {
        let text = record(&[
            "probe 6",
            "opened yes",
            "pages 1",
            "tagged tree yes elements 12 content 5 objects 2",
            "rendered 1",
            "tagged chars matched 40 orphans 1 unmarked 7",
            "ms 5",
            "done",
        ]);
        let tagged = parse_record(&text)
            .expect("complete")
            .tagged
            .expect("a tree");
        assert_eq!(tagged.elements, 12);
        assert_eq!(tagged.content, 5);
        assert_eq!(tagged.objects, 2);
        assert_eq!(tagged.matched, 40);
        assert_eq!(tagged.orphans, 1);
        assert_eq!(tagged.unmarked, 7);
    }

    /// The peak is read where the child printed one, and stays `None` where it
    /// did not -- which is the whole distinction the field exists for. A child
    /// on a platform with no high-water mark to read omits the line, and that
    /// omission becomes a `limits` entry rather than a peak of zero bytes.
    #[test]
    fn a_peak_is_a_measurement_and_its_absence_is_not_a_zero() {
        let lines = ["probe 6", "opened yes", "pages 1", "rendered 1", "ms 5"];
        let mut with = lines.to_vec();
        with.push("peak 21069824");
        with.push("done");
        assert_eq!(
            parse_record(&record(&with)).expect("complete").peak,
            Some(21_069_824)
        );

        let mut without = lines.to_vec();
        without.push("done");
        assert_eq!(
            parse_record(&record(&without)).expect("complete").peak,
            None
        );

        // And a line that is not a number is an absence too, never a zero: a
        // child that garbled its own measurement did not measure nothing.
        let mut broken = lines.to_vec();
        broken.push("peak lots");
        broken.push("done");
        assert_eq!(parse_record(&record(&broken)).expect("complete").peak, None);
    }

    /// `tree no` is a measurement -- this engine looked and found no structure
    /// tree -- and it reads as `None`, which is the same value a corpus of
    /// untagged files produces. The distinction that matters is against a
    /// child too old to look at all, and the version check refuses that record
    /// entirely rather than letting it aggregate as untagged.
    #[test]
    fn a_document_with_no_structure_tree_reads_as_none() {
        let text = record(&[
            "probe 6",
            "opened yes",
            "pages 1",
            "tagged tree no",
            "rendered 1",
            "ms 5",
            "done",
        ]);
        assert!(parse_record(&text).expect("complete").tagged.is_none());
    }

    /// A `chars` line with no `tree yes` before it is a child bug. Reading it
    /// would invent a structure tree with zero elements and forty matched
    /// characters, which no document can have.
    #[test]
    fn a_char_count_without_a_tree_invents_nothing() {
        let text = record(&[
            "probe 6",
            "opened yes",
            "pages 1",
            "tagged chars matched 40 orphans 0 unmarked 0",
            "rendered 1",
            "ms 5",
            "done",
        ]);
        assert!(parse_record(&text).expect("complete").tagged.is_none());
    }

    #[test]
    fn a_complete_record_reads() {
        let result = parse_record(GOOD).expect("it is complete");
        assert_eq!(result.outcome, Outcome::Passed);
        assert_eq!(result.pages, 3);
        assert_eq!(result.rendered, 3);
        assert_eq!(result.warnings["render:UnreadableFont"], 2);
        assert!(result.capabilities.contains("jbig2"));
        assert!(result.degraded());
        assert!(result.strict.eligible());
        assert!(
            result.strict.clean(),
            "a rewrite with no structural defect is clean, whatever it inherited: {:?}",
            result.strict
        );
    }

    /// The strict pass has three answers and they are three different
    /// facts: it did not run, it ran and found something the writer owns,
    /// or it ran and found only what came in with the source.
    #[test]
    fn the_strict_pass_reports_its_own_three_outcomes() {
        let skipped = GOOD.replace(
            "strict eligible\nstrict structure 0\n",
            "strict ineligible the file is encrypted\n",
        );
        let result = parse_record(&skipped).expect("it is complete");
        assert!(!result.strict.eligible());
        assert!(!result.strict.clean(), "not measured is not clean");
        assert!(
            matches!(&result.strict, Strict::Ineligible(why) if why.contains("encrypted")),
            "{:?}",
            result.strict
        );

        let broken = GOOD.replace("strict structure 0", "strict structure 3");
        let result = parse_record(&broken).expect("it is complete");
        assert!(result.strict.eligible());
        assert!(!result.strict.clean());

        // A record that never mentions the pass is a child that skipped
        // it, which is a defect in the child rather than a fact about the
        // file — so it is not eligible and says so.
        let silent: Vec<&str> = GOOD
            .lines()
            .filter(|line| !line.trim_start().starts_with("strict"))
            .collect();
        let silent = format!("{}\n", silent.join("\n"));
        let result = parse_record(&silent).expect("it is complete");
        assert!(!result.strict.eligible());
    }

    /// The sentinel is the isolation. Without it a child killed halfway
    /// through a page reads as a file that rendered every page it got to,
    /// which is the exact shape of a hang counted as a pass.
    #[test]
    fn a_record_without_its_sentinel_is_not_a_record() {
        let truncated = GOOD.replace("done\n", "");
        assert!(parse_record(&truncated).is_none());
        let cut = "probe 6\nopened yes\npages 3\nrendered 1\n";
        assert!(parse_record(cut).is_none());
    }

    #[test]
    fn a_record_in_an_unknown_format_is_refused() {
        assert!(parse_record(&GOOD.replace("probe 6", "probe 8")).is_none());
        // And the version the strict pass replaced: a record without that
        // pass means something else by the same keys.
        assert!(parse_record(&GOOD.replace("probe 6", "probe 1")).is_none());
        assert!(parse_record(&GOOD.replace("probe 6\n", "")).is_none());
    }

    #[test]
    fn a_file_that_would_not_open_is_a_failure_and_not_a_crash() {
        let text = "probe 6\nfile x.pdf\nopened no not a PDF: no indirect objects\nms 2\ndone\n";
        let result = parse_record(text).expect("it is complete");
        assert!(
            matches!(&result.outcome, Outcome::Failed(reason) if reason.contains("not a PDF")),
            "{:?}",
            result.outcome
        );
    }

    /// Ruling 2. A placeholder is correct behaviour, so a page carrying one
    /// passed; it is degraded, which is the other number.
    #[test]
    fn a_degraded_page_passed() {
        let text = "probe 6\nopened yes\npages 1\nrendered 1\n\
                    warn render:UnsupportedImage(JBIG2Decode) 1\ncap jbig2\nms 5\ndone\n";
        let result = parse_record(text).expect("it is complete");
        assert_eq!(result.outcome, Outcome::Passed);
        assert!(result.degraded());
    }

    #[test]
    fn a_page_that_produced_nothing_did_not_pass() {
        let text = "probe 6\nopened yes\npages 4\nrendered 2\nms 5\ndone\n";
        let result = parse_record(text).expect("it is complete");
        assert!(
            matches!(&result.outcome, Outcome::Failed(reason) if reason.contains("2 of 4")),
            "{:?}",
            result.outcome
        );
    }

    /// A newer child may add keys. An older runner must go on reading the
    /// ones it knows rather than declaring every file a crash.
    #[test]
    fn a_metamorphic_verdict_reads_its_three_states() {
        let text = concat!(
            "probe 6\n",
            "opened yes\n",
            "pages 1\n",
            "rendered 1\n",
            "meta rotate held 12 of 501832\n",
            "meta crop broke 12 of 400 pixels of the crop are not the page under it\n",
            "meta dpi skipped the page is too large to render twice\n",
            "ms 1\n",
            "done\n",
        );
        let result = parse_record(text).expect("a record");
        assert_eq!(
            result.metamorphic["rotate"],
            MetaVerdict::Held("12 of 501832".to_string())
        );
        assert!(result.metamorphic["rotate"].held());
        assert!(result.metamorphic["rotate"].compared());

        assert!(!result.metamorphic["crop"].held());
        assert!(
            result.metamorphic["crop"].compared(),
            "a relation that broke was still asked"
        );
        assert!(result.metamorphic["crop"].detail().contains("400 pixels"));

        assert!(
            !result.metamorphic["dpi"].compared(),
            "a relation that was skipped was not asked"
        );
        assert!(!result.metamorphic["dpi"].held());
    }

    /// A child that predates the measurement still says its relation held.
    ///
    /// The bare word is the older shape and it must keep counting, because the
    /// alternative — reading `held` with nothing after it as a verdict this
    /// runner does not know — turns every hold of an older binary into a skip
    /// and empties the denominators the ratchet compares.
    #[test]
    fn a_hold_with_no_measurement_is_still_a_hold() {
        let text = concat!(
            "probe 6\n",
            "opened yes\n",
            "pages 1\n",
            "rendered 1\n",
            "meta rotate held\n",
            "ms 1\n",
            "done\n",
        );
        let result = parse_record(text).expect("a record");
        assert!(result.metamorphic["rotate"].held());
        assert!(result.metamorphic["rotate"].compared());
        assert_eq!(
            result.metamorphic["rotate"].detail(),
            "",
            "a record that did not measure reports no measurement"
        );
    }

    /// A killed child still says who wrote the file.
    ///
    /// This is the producer table's whole usefulness. Its first run over the
    /// production corpus attributed all eighteen non-passing files to
    /// "(unread)", because fourteen were killed at the timeout with the
    /// producer line already written and this runner threw the partial record
    /// away whole. The line is a property of the document and is printed
    /// before any of the work that then did not finish.
    #[test]
    fn a_producer_survives_a_record_that_never_finished() {
        let partial = concat!(
            "probe 6\n",
            "file x.pdf\n",
            "opened yes\n",
            "producer Microsoft(R) PowerPoint(R) for Microsoft 365\n",
            "pages 1\n",
            "phase render\n",
        );
        assert!(
            parse_record(partial).is_none(),
            "a record with no `done` is not a record"
        );
        assert_eq!(
            producer_line(partial).as_deref(),
            Some("Microsoft(R) PowerPoint(R) for Microsoft 365")
        );
        assert_eq!(
            producer_line("probe 6\nopened no nope\n"),
            None,
            "a file that never opened states no producer"
        );
    }

    /// A verdict this runner does not know is **not** a hold.
    ///
    /// The direction matters, and it is the version check's: a newer child that
    /// grows a fourth verdict must not have it read as a success by an older
    /// runner, because that is the reading that turns a regression into a green
    /// tick.
    #[test]
    fn an_unknown_metamorphic_verdict_is_not_a_hold() {
        let text = concat!(
            "probe 6\n",
            "opened yes\n",
            "pages 1\n",
            "rendered 1\n",
            "meta rotate inconclusive\n",
            "ms 1\n",
            "done\n",
        );
        let result = parse_record(text).expect("a record");
        assert!(!result.metamorphic["rotate"].held());
        assert!(!result.metamorphic["rotate"].compared());
    }

    #[test]
    fn an_unknown_key_is_ignored() {
        let text = GOOD.replace("ms 40", "colour_space_hits 12\nms 40");
        let result = parse_record(&text).expect("it is complete");
        assert_eq!(result.outcome, Outcome::Passed);
    }
}
