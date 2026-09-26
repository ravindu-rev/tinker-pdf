//! The corpus sub-commands' command lines.

use std::collections::{BTreeMap, BTreeSet};
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

/// Where the no-faces bar lives, relative to the repository root.
pub const RATCHET_PATH: &str = "corpus/ratchet.json";

/// Where the with-faces bar lives.
///
/// A second file rather than a second section of the first, and the reason is
/// the one [`crate::ratchet`]'s module note gives: the two are different
/// measurements and mixing them makes the ratchet meaningless. Two files means
/// a run cannot even *read* the wrong bar, and the comparator's existing
/// refusal — a bar recorded under one `--fonts` setting will not compare
/// against a run under another — becomes the second lock rather than the only
/// one.
pub const RATCHET_FONTS_PATH: &str = "corpus/ratchet-fonts.json";

/// The `--fonts` value that means "the face this repository writes for
/// itself".
///
/// A keyword rather than a path. A directory of this name would be shadowed,
/// which is stated in `--help`; the alternative is a second flag that can
/// disagree with the first, and a run measured against a face nobody can
/// identify afterwards is not a measurement.
pub const SYNTHETIC_FONTS: &str = "synthetic";

/// The `--fonts` value that means "the faces the child's own build carries".
///
/// Passed through to `tpdf`, which refuses it unless it was built with
/// `bundled-fonts` — so this cannot quietly record a no-faces run as the
/// bundled bar. The setting names the family and its version, because a
/// different face set is a different measurement and the comparator's refusal
/// works on the setting string.
pub const BUNDLED_FONTS: &str = "bundled";

/// What a bundled run records as its `--fonts` setting.
pub const BUNDLED_SETTING: &str = "bundled-liberation-2.1.5";

/// Where the bundled-faces bar lives.
pub const RATCHET_BUNDLED_PATH: &str = "corpus/ratchet-bundled.json";

/// Which bar a run compares against, from what it was measured with.
#[must_use]
pub fn ratchet_path(fonts: &str) -> &'static str {
    match fonts {
        "none" => RATCHET_PATH,
        BUNDLED_SETTING => RATCHET_BUNDLED_PATH,
        _ => RATCHET_FONTS_PATH,
    }
}

/// What a run was measured with, and where the child should look for it.
///
/// Returns the name the report records and the path the child is given.
fn resolve_fonts(root: &Path, args: &RunArgs) -> Result<(String, Option<String>), String> {
    match args.fonts.as_deref() {
        None => Ok(("none".to_string(), None)),
        Some(SYNTHETIC_FONTS) => {
            let path = crate::face::default_path(root);
            crate::face::write(&path)?;
            eprintln!("corpus-run: wrote the synthetic face to {}", path.display());
            Ok((
                crate::face::SYNTHETIC.to_string(),
                Some(path.display().to_string()),
            ))
        }
        Some(BUNDLED_FONTS) => Ok((BUNDLED_SETTING.to_string(), Some(BUNDLED_FONTS.to_string()))),
        Some(path) => Ok((path.to_string(), Some(path.to_string()))),
    }
}

/// Everything `corpus-run`'s command line can say.
#[derive(Clone, Debug)]
pub struct RunArgs {
    /// Only these corpora, by lock name. Empty means all of them.
    pub only: Vec<String>,
    /// The per-file timeout.
    ///
    /// Sixty seconds from 4-5 September 2026, up from twenty. The metamorphic
    /// relations used to be declined for any file that had already spent
    /// 3 100 ms, precisely so that the extra work could not run twenty seconds
    /// out — and that clock decided a ratcheted denominator, so the nightly
    /// failed for a week on counts that moved with the runner's load. Deleting
    /// the gate means the relations are always asked, which needs the room.
    ///
    /// **Three minutes from 6 September 2026, and this time the number is
    /// sited on the whole distribution rather than on the two files that were
    /// flipping.** Sixty was set against a corpus whose slowest file took
    /// 20 s. It is now measured per corpus, on an idle machine, after the
    /// rasteriser's row loop was fixed:
    ///
    /// | Corpus | Slowest file | What it is |
    /// | --- | ---: | --- |
    /// | `pdfjs` | 54 s | `issue16263.pdf` |
    /// | `verapdf` | 48 s | the 10 000-page implementation-limit fixture |
    /// | `qpdf` | 21 s | `numeric-and-string-2.pdf` |
    /// | `pdfa-examples` | 0.2 s | |
    /// | `safedocs` | 80 s | `0000231.pdf`, 4.5 MB off the open web |
    ///
    /// Two of those sat inside a factor of 1.25 of sixty seconds, which is
    /// the hazard this repository has already been bitten by twice: a file
    /// near the limit is passed on an idle run and killed on a busy one, the
    /// pass rate moves, and every ratcheted count that file contributed to
    /// moves with it. A limit has to clear the slowest *legitimate* file by
    /// enough to survive a shared runner, and three minutes is between three
    /// and nine times each of these.
    ///
    /// It is still a limit and still catches a hang: the stall detector fires
    /// on a child that has written nothing for half its budget, and no file
    /// measured here is silent for ninety seconds.
    pub timeout: Duration,
    /// Whether the caller named `--timeout` on the command line.
    ///
    /// The distinction matters because a corpus may state its own in
    /// `corpora.lock`, and the two have to be ordered. An explicit flag is an
    /// *override* of the whole run and wins; the default is only a default,
    /// and a corpus that states a timeout of its own beats it.
    pub timeout_explicit: bool,
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
            timeout: Duration::from_secs(180),
            timeout_explicit: false,
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
                    out.timeout_explicit = true;
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
    let (fonts, fonts_path) = resolve_fonts(root, &args)?;
    let child = resolve_child(&args, fonts_path.as_deref())?;

    // **The sidecar is read before the run, and a defect in it stops the
    // run.** Thirty-four files open only because of it, so a sidecar that
    // will not parse is thirty-four failures the pass rate would carry
    // without saying why — the same shape as the stale-binary check above.
    let passwords_path = root.join(crate::passwords::PASSWORDS_PATH);
    let passwords = match std::fs::read_to_string(&passwords_path) {
        Ok(text) => crate::passwords::parse(&text)
            .map_err(|e| format!("{}: {e}", crate::passwords::PASSWORDS_PATH))?,
        // Absent is allowed: `--child` may name someone else's program and a
        // checkout may be partial. It is recorded as a limit rather than
        // passed over, because a run without it measures thirty-four files
        // differently and a bar is a comparison between runs.
        Err(_) => crate::passwords::Passwords::default(),
    };
    let known: Vec<String> = corpora.iter().map(|c| c.name.clone()).collect();
    let stale = passwords.corpora_not_in(&known);
    if !stale.is_empty() {
        return Err(format!(
            "{} names {} corpus this lockfile does not have ({}); a row that \
             matches no corpus is silent, which is why it is refused here",
            crate::passwords::PASSWORDS_PATH,
            stale.len(),
            stale
                .iter()
                .map(|row| format!("{}/{}", row.corpus, row.path))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let mut limits: Vec<String> = Vec::new();
    if passwords.rows().is_empty() {
        limits.push(format!(
            "no passwords were supplied: {} was not read, so every \
             password-protected file is a failure rather than a measurement",
            crate::passwords::PASSWORDS_PATH
        ));
    }
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

        // **The timeout this corpus is measured with.** A per-corpus value in
        // the lock is a statement about the documents; `--timeout` on the
        // command line is a statement about the run, so it wins.
        let timeout = match (args.timeout_explicit, corpus.timeout_seconds) {
            (false, Some(seconds)) => Duration::from_secs(seconds),
            _ => args.timeout,
        };
        if timeout != args.timeout {
            // Said out loud, because a pass rate measured at a different
            // timeout is a different measurement and the log is where anybody
            // reading a moved bar starts.
            eprintln!(
                "corpus-run: {} — {} files, {} s per file (its own, from the lock)",
                corpus.name,
                files.len(),
                timeout.as_secs()
            );
        } else {
            eprintln!("corpus-run: {} — {} files", corpus.name, files.len());
        }
        let for_corpus = passwords.for_corpus(&corpus.name);
        let results = run_files(&child, &dir, &files, timeout, args.jobs, &for_corpus);

        // **A sidecar row that matched no file is a stale row**, and a stale
        // row is silent: the file it names was renamed or removed upstream,
        // nothing is passed to any child, and the report shows only that some
        // file failed for a password. On a sampled run it means nothing —
        // most files were not run — so it is only looked for on a whole one.
        if args.sample.is_none() {
            let seen: BTreeSet<&str> = results.iter().map(|r| r.path.as_str()).collect();
            let unmatched: Vec<&str> = for_corpus
                .keys()
                .copied()
                .filter(|path| !seen.contains(path))
                .collect();
            if !unmatched.is_empty() {
                limits.push(format!(
                    "`{}` has {} password row(s) matching no file: {} — {} is \
                     stale against this pin",
                    corpus.name,
                    unmatched.len(),
                    unmatched.join(", "),
                    crate::passwords::PASSWORDS_PATH
                ));
            }
        }

        let report = CorpusReport {
            name: corpus.name.clone(),
            files: results,
        };
        // **A corpus nobody could measure the memory of is an incomplete run.**
        // The child reads its own high-water mark on Linux and Windows and
        // omits the line everywhere else, so a corpus where not one child
        // reported a peak is a platform that cannot answer rather than a
        // corpus that costs nothing. Saying so in `limits` is what makes
        // `ratchet::compare` refuse: without it the maximum would be a silent
        // zero, and a band of zero bytes is one nothing can sit under.
        //
        // Not one child rather than every child, because a file that crashed
        // wrote no record at all and would otherwise make every real run
        // incomplete over a failure the pass rate already counts.
        if report.total() > 0 && report.peak().files == 0 {
            limits.push(format!(
                "`{}` has no memory measurement: not one child reported a peak \
                 resident set, which is a platform `tpdf` cannot read one on",
                corpus.name
            ));
        }
        reports.push(report);
    }

    let run = Run {
        corpora: reports,
        limits,
        settings: Settings {
            timeout_seconds: args.timeout.as_secs(),
            dpi: args.dpi,
            fonts,
        },
    };

    // **The child's faces and the run's setting must agree.** A `tpdf` built
    // with `bundled-fonts` measures every file with twelve Liberation faces
    // whether or not anybody asked, so a plain run against that binary
    // produces the bundled numbers and would record them as the no-faces bar —
    // a silent 52 % improvement that is not one. The child says which it is,
    // in its own record, from its own `cfg`; this is where the two are
    // compared, and it refuses rather than resolves.
    let carried = run
        .corpora
        .iter()
        .flat_map(|corpus| corpus.files.iter())
        .any(|file| file.bundled_faces);
    let asked = run.settings.fonts == crate::corpus::BUNDLED_SETTING;
    if carried != asked {
        return Err(if carried {
            format!(
                "the child was built with `bundled-fonts`, so every file was measured with twelve faces, and this run is recorded as `{}`. Rebuild `tpdf` without the feature, or run with `--fonts bundled`.",
                run.settings.fonts
            )
        } else {
            "`--fonts bundled` was asked for and the child carries no faces; rebuild `tpdf` with `--features bundled-fonts`"
                .to_string()
        });
    }

    for line in run.summary_lines() {
        println!("{line}");
    }
    println!();
    print!("{}", run.capability_table());
    // Only when there is something to attribute: `producer_table` returns an
    // empty string for a run where everything passed.
    let attribution = run.producer_table();
    if !attribution.is_empty() {
        println!();
        print!("{attribution}");
    }

    if let Some(path) = &args.report {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, run.to_report_json().to_pretty())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        println!("\nwrote {}", path.display());
    }

    let bar_path = ratchet_path(&run.settings.fonts);
    let ratchet_path = root.join(bar_path);
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
        let bar = ratchet::parse(&text).map_err(|e| format!("{bar_path}: {e}"))?;
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
            // With the flags that produced it: two bars now exist, and a
            // hint that re-records the wrong one is worse than none.
            let again = match args.fonts.as_deref() {
                None => String::new(),
                Some(fonts) => format!(" --fonts {fonts}"),
            };
            println!(
                "\nthe bar held, and moved. To take the new numbers:\n  \
                 cargo run -p xtask -- corpus-run --record{again}"
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
         engine. Re-measure with --fonts synthetic to see the other number."
    } else if run.settings.fonts == crate::face::SYNTHETIC {
        "WITH the face this repository writes for itself \
         (`cargo xtask synth-face`): every glyph from 32 up is the same \
         filled box, so it answers `was a face available` and nothing \
         else. The difference between this bar and the no-faces one is \
         how much of the degradation was the absence of a face rather \
         than a defect in the engine; neither figure says the text was \
         set correctly. The face is versioned in this setting, so \
         changing it invalidates the bar rather than silently moving \
         it. Not comparable with a no-faces figure either."
    } else {
        "WITH font faces supplied, so `degraded` excludes the missing-face \
         term. It is not comparable with a no-faces figure and the comparator \
         refuses to try."
    };
    format!(
        "Pass rate per corpus. `passed` means a bitmap came back for every \
         page without crashing or timing out (ruling 2) — a placeholder \
         counts. `degraded` is the second axis: files that rendered with \
         something reported. Measured {faces} `strict_eligible` and \
         `strict_clean` are ruling 13's third axis, and they are about the \
         *writer*: every file this engine read cleanly is rewritten in \
         memory and the rewrite is validated against ISO 32000 read \
         strictly. Only the structural tier counts — the header, the \
         cross-reference sections, the offsets, the stream extents, the \
         trailer — because a rewrite copies the page tree, the \
         annotations and the resource dictionaries from its source, and a \
         defect there belongs to the source. A file this engine could not \
         read cleanly is not eligible and is counted as neither. `metamorphic` is the fourth axis (roadmap step 7): relations that must hold between two renders of one file, which need no ground truth. `rotate` turns the page a quarter and requires the transposition; `crop` moves the page box and requires the sub-rectangle of the full render, exactly; `dpi` renders at twice the scale, box-filters down and requires agreement. Each records `compared` beside `held`, because a relation that declines the hard files is not a relation that held — and `rotate` and `crop` are asked only of files this engine read cleanly, since a rewrite of a repaired document compares two repairs. What none of them catches is a defect that commutes with the transformation: a colour converted wrongly is converted equally wrongly at both resolutions and both rotations, and every row stays green. Compared \
         by integer cross-multiplication, never as floats: \
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
/// `passwords` maps a file's report path to the password to open it with, and
/// is the corpus's own slice of `corpus/passwords.tsv`. A file with no row is
/// run exactly as before.
pub fn run_files(
    child: &Child,
    dir: &Path,
    files: &[PathBuf],
    timeout: Duration,
    jobs: usize,
    passwords: &BTreeMap<&str, &str>,
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
                let extra = match passwords.get(relative.as_str()) {
                    Some(password) => {
                        vec!["--password".to_string(), (*password).to_string()]
                    }
                    None => Vec::new(),
                };
                let result = runner::run_one(child, file, &relative, timeout, &extra);
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
fn resolve_child(args: &RunArgs, fonts: Option<&str>) -> Result<Child, String> {
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
    if let Some(fonts) = fonts {
        child_args.push("--fonts".to_string());
        child_args.push(fonts.to_string());
    }
    let child = Child {
        program: sibling,
        args: child_args,
    };
    agrees_on_the_record_format(&child)?;
    Ok(child)
}

/// Refuses a child whose record format this runner does not read.
///
/// Asked once, before the run, because the alternative is what it replaced: a
/// child one version behind writes a complete record per file, the runner
/// refuses each one as unreadable, and four thousand refusals arrive as
/// `0/4525 passed` — which is indistinguishable from an engine that stopped
/// rendering, and sends whoever reads it looking for a rendering bug that is
/// not there. A stale binary is a different fact and now says so.
///
/// A child that does not understand the question at all is let through rather
/// than refused: `--child` may name someone else's program, and this runner
/// has no standing to require a flag of it. The per-file version check still
/// catches a mismatch; this only makes the common case legible.
fn agrees_on_the_record_format(child: &Child) -> Result<(), String> {
    let asked = std::process::Command::new(&child.program)
        .arg("probe")
        .arg("--record-version")
        .output();
    let Ok(output) = asked else {
        return Ok(());
    };
    if !output.status.success() {
        return Ok(());
    }
    match record_version_disagreement(&child.program, &String::from_utf8_lossy(&output.stdout)) {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

/// The decision [`agrees_on_the_record_format`] makes, separated from the
/// spawn so it can be tested without a binary to spawn.
fn record_version_disagreement(program: &Path, said: &str) -> Option<String> {
    let version = said
        .lines()
        .find_map(|line| line.trim().strip_prefix("probe "))
        .and_then(|rest| rest.trim().parse::<u32>().ok())?;
    if version == runner::PROBE_VERSION {
        return None;
    }
    Some(format!(
        "{} writes probe records at version {version} and this runner reads version {}. Rebuild it: `cargo build -p tpdf` (add --release if this xtask is a release build).",
        program.display(),
        runner::PROBE_VERSION
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The failure this exists to replace: a child one version behind used to
    /// produce four thousand unreadable records, which arrived as `0/4525
    /// passed` — a message about rendering, for a problem about a binary.
    #[test]
    fn a_child_a_version_behind_is_named_rather_than_counted_as_a_regression() {
        let stale = record_version_disagreement(
            Path::new("target/release/tpdf"),
            &format!(
                "probe {}
",
                runner::PROBE_VERSION - 1
            ),
        )
        .expect("a version behind is a refusal");
        assert!(stale.contains("writes probe records at version"), "{stale}");
        assert!(stale.contains("cargo build -p tpdf"), "{stale}");
        assert!(
            record_version_disagreement(
                Path::new("tpdf"),
                &format!(
                    "probe {}
",
                    runner::PROBE_VERSION
                ),
            )
            .is_none(),
            "the version this runner reads is not a disagreement"
        );
    }

    /// A `--child` naming somebody else's program owes this runner no flag,
    /// so an answer it cannot read is let through. The per-file version check
    /// still catches a real mismatch; the preflight only makes the common
    /// case legible, and refusing on silence would make it a requirement.
    #[test]
    fn a_child_that_does_not_answer_is_let_through() {
        for said in ["", "usage: someprog [options]", "probe", "probe next"] {
            assert!(
                record_version_disagreement(Path::new("other"), said).is_none(),
                "{said:?}"
            );
        }
    }

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
        assert!(
            args.timeout_explicit,
            "a `--timeout` on the command line overrides a corpus's own"
        );
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
