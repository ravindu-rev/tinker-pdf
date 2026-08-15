//! The corpus sub-commands' command lines.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::report::{CorpusReport, Run, Settings};
use crate::runner::{self, Child};
use crate::{lock, ratchet};

/// `cargo xtask corpus-licences` — plan 14's table, generated from the lock.
///
/// `--check` compares it against the committed `corpus/README.md` and fails on
/// a difference, so a corpus cannot be added without its licence reaching the
/// file a person reads.
pub fn licences(root: &Path, args: &[String]) -> Result<(), String> {
    let check = args.iter().any(|a| a == "--check");
    let corpora = lock::read(root)?;
    let table = lock::licence_table(&corpora);

    if !check {
        print!("{table}");
        return Ok(());
    }

    let readme_path = root.join("corpus/README.md");
    let readme = std::fs::read_to_string(&readme_path)
        .map_err(|e| format!("{}: {e}", readme_path.display()))?;
    let missing: Vec<&str> = table
        .lines()
        .filter(|line| !readme.contains(*line))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "corpus/README.md has drifted from the lock. It is missing:\n{}\n\
         Run `cargo xtask corpus-licences` and paste the table in.",
        missing.join("\n")
    ))
}

/// Where `ratchet.json` lives, relative to the repository root.
pub const RATCHET_PATH: &str = "corpus/ratchet.json";

/// Everything `corpus-run`'s command line can say.
#[derive(Clone, Debug)]
pub struct RunArgs {
    /// Only these corpora, by lock name. Empty means all of them.
    pub only: Vec<String>,
    /// The per-file timeout.
    pub timeout: Duration,
    /// Render resolution.
    pub dpi: f64,
    /// A face or directory of faces for documents that embed none.
    pub fonts: Option<String>,
    /// How many files to run at once.
    pub jobs: usize,
    /// At most this many files per corpus. Recorded as a limit.
    pub sample: Option<usize>,
    /// The child to spawn. Defaults to `tpdf` beside this executable.
    pub child: Option<PathBuf>,
    /// Where to write the full per-file report.
    pub report: Option<PathBuf>,
    /// Compare against the committed ratchet and fail on a regression.
    pub check: bool,
    /// Rewrite the committed ratchet from this run.
    pub record: bool,
    /// Treat a rise in the degradation rate as a regression too.
    pub strict: bool,
}

impl Default for RunArgs {
    fn default() -> RunArgs {
        RunArgs {
            only: Vec::new(),
            timeout: Duration::from_secs(20),
            // 72 dpi: one device pixel per point. The question this run asks
            // is whether a bitmap comes back at all, and asking it four times
            // over at 150 costs hours across four thousand files without
            // changing a single answer.
            dpi: 72.0,
            fonts: None,
            jobs: std::thread::available_parallelism().map_or(4, |n| n.get()),
            sample: None,
            child: None,
            report: None,
            check: false,
            record: false,
            strict: false,
        }
    }
}

impl RunArgs {
    pub fn parse(args: &[String]) -> Result<RunArgs, String> {
        let mut out = RunArgs::default();
        let mut index = 0;
        while index < args.len() {
            let arg = args[index].as_str();
            let mut value = || -> Result<String, String> {
                index += 1;
                args.get(index)
                    .cloned()
                    .ok_or_else(|| format!("`{arg}` needs a value"))
            };
            match arg {
                "--corpus" => out.only.push(value()?),
                "--timeout" => {
                    let raw = value()?;
                    let seconds: u64 = raw
                        .parse()
                        .map_err(|_| format!("`--timeout {raw}` is not a number of seconds"))?;
                    if seconds == 0 {
                        return Err("`--timeout 0` would kill every child instantly".to_string());
                    }
                    out.timeout = Duration::from_secs(seconds);
                }
                "--dpi" => {
                    let raw = value()?;
                    let dpi: f64 = raw
                        .parse()
                        .map_err(|_| format!("`--dpi {raw}` is not a number"))?;
                    if !dpi.is_finite() || dpi <= 0.0 {
                        return Err(format!("`--dpi {raw}` is not a resolution"));
                    }
                    out.dpi = dpi;
                }
                "--fonts" => out.fonts = Some(value()?),
                "--jobs" => {
                    let raw = value()?;
                    out.jobs = raw
                        .parse::<usize>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or_else(|| format!("`--jobs {raw}` is not a positive number"))?;
                }
                "--sample" => {
                    let raw = value()?;
                    out.sample = Some(
                        raw.parse::<usize>()
                            .ok()
                            .filter(|n| *n > 0)
                            .ok_or_else(|| format!("`--sample {raw}` is not a positive number"))?,
                    );
                }
                "--child" => out.child = Some(PathBuf::from(value()?)),
                "--report" => out.report = Some(PathBuf::from(value()?)),
                "--check" => out.check = true,
                "--record" => out.record = true,
                "--strict" => out.strict = true,
                other => return Err(format!("unknown option `{other}`")),
            }
            index += 1;
        }
        if out.check && out.record {
            return Err(
                "`--check` and `--record` disagree: one holds the bar, the other \
                 moves it"
                    .to_string(),
            );
        }
        Ok(out)
    }
}

/// `cargo xtask corpus-run`.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let args = RunArgs::parse(args)?;
    let corpora = lock::read(root)?;
    let child = resolve_child(&args)?;

    let mut limits: Vec<String> = Vec::new();
    if let Some(sample) = args.sample {
        limits.push(format!(
            "sampled: at most {sample} files per corpus were run, in path order"
        ));
    }

    let selected: Vec<&lock::Corpus> = corpora
        .iter()
        .filter(|c| args.only.is_empty() || args.only.contains(&c.name))
        .collect();
    for name in &args.only {
        if !corpora.iter().any(|c| c.name == *name) {
            return Err(format!("`{name}` is not a corpus in {}", lock::LOCK_PATH));
        }
    }
    if !args.only.is_empty() {
        limits.push(format!(
            "only these corpora were run: {}",
            args.only.join(", ")
        ));
    }

    let mut reports = Vec::new();
    for corpus in &selected {
        let dir = lock::files_dir(root, corpus);
        let mut files = pdfs_under(&dir);
        files.sort();

        if files.is_empty() {
            // Visible, in the report, and fatal to any comparison. A corpus
            // that is not on disk is the single most likely way this job
            // passes while measuring nothing, so it is never a quiet zero.
            limits.push(format!(
                "`{}` was not run: no PDFs under {} — fetch it with \
                 `cargo xtask corpus-fetch`",
                corpus.name,
                dir.display()
            ));
            eprintln!(
                "corpus-run: `{}` has no files under {}",
                corpus.name,
                dir.display()
            );
            reports.push(CorpusReport {
                name: corpus.name.clone(),
                files: Vec::new(),
            });
            continue;
        }

        if let Some(sample) = args.sample {
            if files.len() > sample {
                files.truncate(sample);
            }
        }

        eprintln!("corpus-run: {} — {} files", corpus.name, files.len());
        let results = run_files(&child, &dir, &files, args.timeout, args.jobs);
        reports.push(CorpusReport {
            name: corpus.name.clone(),
            files: results,
        });
    }

    let run = Run {
        corpora: reports,
        limits,
        settings: Settings {
            timeout_seconds: args.timeout.as_secs(),
            dpi: args.dpi,
            fonts: args.fonts.clone().unwrap_or_else(|| "none".to_string()),
        },
    };

    for line in run.summary_lines() {
        println!("{line}");
    }
    println!();
    print!("{}", run.capability_table());

    if let Some(path) = &args.report {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, run.to_report_json().to_pretty())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        println!("\nwrote {}", path.display());
    }

    let ratchet_path = root.join(RATCHET_PATH);
    if args.record {
        let note = ratchet_note(&run);
        std::fs::write(&ratchet_path, run.to_ratchet_json(&note).to_pretty())
            .map_err(|e| format!("{}: {e}", ratchet_path.display()))?;
        println!("recorded {}", ratchet_path.display());
        // Recording an incomplete run is allowed only in the sense that the
        // file will say so and nothing will ever be able to compare against
        // it. Saying that here saves somebody a confusing CI failure later.
        if !run.complete() {
            return Err(
                "the run was incomplete, so the ratchet it recorded is marked \
                 incomplete and cannot be a bar"
                    .to_string(),
            );
        }
        return Ok(());
    }

    if args.check {
        let text = std::fs::read_to_string(&ratchet_path)
            .map_err(|e| format!("{}: {e}", ratchet_path.display()))?;
        let bar = ratchet::parse(&text).map_err(|e| format!("{RATCHET_PATH}: {e}"))?;
        let comparison = ratchet::compare(&bar, &run, args.strict);

        for note in &comparison.notes {
            println!("note: {note}");
        }
        for improvement in &comparison.improvements {
            println!("better: {improvement}");
        }
        if comparison.failed() {
            let mut message = String::new();
            for refusal in &comparison.refusals {
                message.push_str(&format!("\n  refused: {refusal}"));
            }
            for regression in &comparison.regressions {
                message.push_str(&format!("\n  regression: {regression}"));
            }
            return Err(format!("the corpus ratchet did not hold:{message}"));
        }
        if !comparison.improvements.is_empty() {
            println!(
                "\nthe bar held, and moved. To take the new numbers:\n  \
                 cargo run -p xtask -- corpus-run --record"
            );
        }
    }
    Ok(())
}

/// The note carried in `ratchet.json`, which is the first thing anybody reads.
fn ratchet_note(run: &Run) -> String {
    let faces = if run.settings.fonts == "none" {
        "WITHOUT font faces: this engine bundles none, so `degraded` here is \
         dominated by documents that embed no font and therefore draw no text. \
         It is a fact about this build's font policy as much as about the \
         engine. Re-measure with --fonts to see the other number."
    } else {
        "WITH font faces supplied, so `degraded` excludes the missing-face \
         term. It is not comparable with a no-faces figure and the comparator \
         refuses to try."
    };
    format!(
        "Pass rate per corpus. `passed` means a bitmap came back for every \
         page without crashing or timing out (ruling 2) — a placeholder \
         counts. `degraded` is the second axis: files that rendered with \
         something reported. Measured {faces} Compared by integer \
         cross-multiplication, never as floats: \
         passed_now * total_before >= passed_before * total_now."
    )
}

/// Runs every file, `jobs` at a time, and returns the results in path order.
///
/// Parallel because four thousand process spawns are otherwise most of an
/// hour, and ordered afterwards because a report whose entries move between
/// runs is not diffable. The timeout is wall clock per child, so a run at a
/// job count far above the core count can time a slow file out that a serial
/// run would not — which is why the default is the core count rather than
/// something greedier.
pub fn run_files(
    child: &Child,
    dir: &Path,
    files: &[PathBuf],
    timeout: Duration,
    jobs: usize,
) -> Vec<runner::FileResult> {
    let next = Arc::new(AtomicUsize::new(0));
    let results = Arc::new(Mutex::new(Vec::with_capacity(files.len())));
    let done = Arc::new(AtomicUsize::new(0));

    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1) {
            let next = Arc::clone(&next);
            let results = Arc::clone(&results);
            let done = Arc::clone(&done);
            scope.spawn(move || loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(file) = files.get(index) else {
                    return;
                };
                let relative = file
                    .strip_prefix(dir)
                    .unwrap_or(file)
                    .to_string_lossy()
                    .replace('\\', "/");
                let result = runner::run_one(child, file, &relative, timeout);
                let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
                if finished % 250 == 0 {
                    eprintln!("corpus-run:   {finished}/{}", files.len());
                }
                results.lock().expect("the results lock").push(result);
            });
        }
    });

    let mut out = Arc::try_unwrap(results)
        .expect("every worker has finished")
        .into_inner()
        .expect("the results lock");
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Every `.pdf` under a directory, recursively.
///
/// By extension, because that is what the corpora themselves mean by a fixture
/// — pdf.js in particular stores `*.pdf.link` files for the fixtures it does
/// not redistribute, and a runner that opened those would report several
/// hundred failures that are URLs.
pub fn pdfs_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = BTreeSet::new();
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if seen.insert(path.clone()) {
                    stack.push(path);
                }
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            {
                out.push(path);
            }
        }
    }
    out
}

/// The child to spawn, which must exist before a run starts.
///
/// Beside this executable rather than searched for on `PATH` or guessed at:
/// `target/debug/xtask` and `target/debug/tpdf` are built by the same command
/// into the same directory, so taking the sibling guarantees the child is the
/// same profile and the same revision as the runner. A `tpdf` found on `PATH`
/// could be anything, and a corpus measured against last month's build that
/// happened to be installed is worse than no measurement.
fn resolve_child(args: &RunArgs) -> Result<Child, String> {
    if let Some(path) = &args.child {
        if !path.exists() {
            return Err(format!("--child {}: no such program", path.display()));
        }
        return Ok(Child {
            program: path.clone(),
            args: Vec::new(),
        });
    }

    let exe = std::env::current_exe()
        .map_err(|e| format!("this executable's own path could not be read: {e}"))?;
    let sibling = exe
        .parent()
        .ok_or("this executable has no directory")?
        .join(if cfg!(windows) { "tpdf.exe" } else { "tpdf" });
    if !sibling.exists() {
        return Err(format!(
            "{} does not exist. corpus-run spawns `tpdf probe`, one process per \
             file; build it first with `cargo build -p tpdf` (add --release if \
             this xtask is a release build).",
            sibling.display()
        ));
    }

    let mut child_args = vec![
        "probe".to_string(),
        "--dpi".to_string(),
        args.dpi.to_string(),
    ];
    if let Some(fonts) = &args.fonts {
        child_args.push("--fonts".to_string());
        child_args.push(fonts.clone());
    }
    Ok(Child {
        program: sibling,
        args: child_args,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_reads() {
        let args = RunArgs::parse(&[
            "--timeout".into(),
            "5".into(),
            "--jobs".into(),
            "2".into(),
            "--corpus".into(),
            "qpdf".into(),
            "--strict".into(),
        ])
        .expect("it parses");
        assert_eq!(args.timeout, Duration::from_secs(5));
        assert_eq!(args.jobs, 2);
        assert_eq!(args.only, vec!["qpdf".to_string()]);
        assert!(args.strict);
    }

    #[test]
    fn nonsense_on_the_command_line_is_refused() {
        for (args, expected) in [
            (vec!["--timeout".to_string(), "0".to_string()], "instantly"),
            (vec!["--jobs".to_string(), "0".to_string()], "positive"),
            (vec!["--sample".to_string(), "x".to_string()], "positive"),
            (vec!["--dpi".to_string(), "-1".to_string()], "resolution"),
            (vec!["--nope".to_string()], "unknown option"),
            (vec!["--timeout".to_string()], "needs a value"),
            (
                vec!["--check".to_string(), "--record".to_string()],
                "disagree",
            ),
        ] {
            let error = RunArgs::parse(&args).expect_err(&format!("{args:?} must be refused"));
            assert!(error.contains(expected), "expected `{expected}`: {error}");
        }
    }

    /// pdf.js stores `name.pdf.link` for fixtures it does not redistribute.
    /// Reading those as documents would add several hundred failures that are
    /// URLs, and they would look exactly like an engine that cannot open a
    /// tenth of pdf.js.
    #[test]
    fn only_files_named_pdf_are_run() {
        let dir = std::env::temp_dir().join(format!("tinker-pdfs-under-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("nested")).expect("a directory");
        for name in ["a.pdf", "b.pdf.link", "c.txt", "D.PDF"] {
            std::fs::write(dir.join(name), b"x").expect("a file");
        }
        std::fs::write(dir.join("nested/e.pdf"), b"x").expect("a file");

        let mut found: Vec<String> = pdfs_under(&dir)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        found.sort();
        assert_eq!(found, vec!["D.PDF", "a.pdf", "e.pdf"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
