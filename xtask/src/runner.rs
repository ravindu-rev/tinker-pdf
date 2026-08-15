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
//! hangs is `TimedOut`; the report carries them and the run continues.
//!
//! ## How a crash is told from a failure
//!
//! By a sentinel, not by an exit code. The child writes `done` as the last
//! line of a complete record, so:
//!
//! - record ends in `done` — the child finished, and the record says what it
//!   found, including that a file would not open;
//! - no `done`, and we killed it — `TimedOut`;
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
    /// The child was still running when the timeout expired, and was killed.
    TimedOut,
}

impl Outcome {
    /// The single word the report and the summary use.
    pub fn label(&self) -> &'static str {
        match self {
            Outcome::Passed => "passed",
            Outcome::Failed(_) => "failed",
            Outcome::Crashed(_) => "crashed",
            Outcome::TimedOut => "timed_out",
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
pub fn run_one(child: &Child, file: &Path, relative: &str, timeout: Duration) -> FileResult {
    let started = Instant::now();
    let empty = |outcome: Outcome, millis: u64| FileResult {
        path: relative.to_string(),
        outcome,
        pages: 0,
        rendered: 0,
        warnings: BTreeMap::new(),
        capabilities: BTreeSet::new(),
        millis,
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

    let mut killed = false;
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
            empty(Outcome::Crashed(detail), millis)
        }
    };

    // A killed child may still have flushed a complete record in the instant
    // before it died; it is a timeout all the same, because the run had
    // already decided not to wait for it and counting it as a pass would make
    // the timeout depend on scheduling luck.
    if killed {
        result.outcome = Outcome::TimedOut;
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
const PROBE_VERSION: u32 = 1;

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
            "cap" => {
                capabilities.insert(rest.trim().to_string());
            }
            "warn" => {
                let (label, count) = rest.rsplit_once(' ').unwrap_or((rest.trim(), "1"));
                let count: usize = count.trim().parse().unwrap_or(1);
                *warnings.entry(label.trim().to_string()).or_default() += count;
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "probe 1\nfile x.pdf\nopened yes\nladder Trust\npages 3\n\
                        cap jbig2\nrendered 3\nwarn render:UnreadableFont 2\nms 40\ndone\n";

    #[test]
    fn a_complete_record_reads() {
        let result = parse_record(GOOD).expect("it is complete");
        assert_eq!(result.outcome, Outcome::Passed);
        assert_eq!(result.pages, 3);
        assert_eq!(result.rendered, 3);
        assert_eq!(result.warnings["render:UnreadableFont"], 2);
        assert!(result.capabilities.contains("jbig2"));
        assert!(result.degraded());
    }

    /// The sentinel is the isolation. Without it a child killed halfway
    /// through a page reads as a file that rendered every page it got to,
    /// which is the exact shape of a hang counted as a pass.
    #[test]
    fn a_record_without_its_sentinel_is_not_a_record() {
        let truncated = GOOD.replace("done\n", "");
        assert!(parse_record(&truncated).is_none());
        let cut = "probe 1\nopened yes\npages 3\nrendered 1\n";
        assert!(parse_record(cut).is_none());
    }

    #[test]
    fn a_record_in_an_unknown_format_is_refused() {
        assert!(parse_record(&GOOD.replace("probe 1", "probe 2")).is_none());
        assert!(parse_record(&GOOD.replace("probe 1\n", "")).is_none());
    }

    #[test]
    fn a_file_that_would_not_open_is_a_failure_and_not_a_crash() {
        let text = "probe 1\nfile x.pdf\nopened no not a PDF: no indirect objects\nms 2\ndone\n";
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
        let text = "probe 1\nopened yes\npages 1\nrendered 1\n\
                    warn render:UnsupportedImage(JBIG2Decode) 1\ncap jbig2\nms 5\ndone\n";
        let result = parse_record(text).expect("it is complete");
        assert_eq!(result.outcome, Outcome::Passed);
        assert!(result.degraded());
    }

    #[test]
    fn a_page_that_produced_nothing_did_not_pass() {
        let text = "probe 1\nopened yes\npages 4\nrendered 2\nms 5\ndone\n";
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
    fn an_unknown_key_is_ignored() {
        let text = GOOD.replace("ms 40", "colour_space_hits 12\nms 40");
        let result = parse_record(&text).expect("it is complete");
        assert_eq!(result.outcome, Outcome::Passed);
    }
}
