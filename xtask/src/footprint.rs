//! The footprint ratchet: each build profile's gzipped wasm, held to a
//! committed figure plus a measured tolerance.
//!
//! # Why this exists
//!
//! The release was the only thing that measured the wasm. `release.yml`'s
//! npm job fails above a 2.5 MiB gzipped budget, and nothing between two
//! releases looked at the size at all, so the work between releases crossed
//! the budget without anybody seeing which commit did it
//! (`docs/features/bindings.md` dates the sizes). A figure held on every push
//! names the commit that moved the number while it is still the newest one.
//!
//! # Why the size is passed in
//!
//! The workflow builds each profile as the release builds it, measures it as
//! the release measures it, `gzip -9 -c … | wc -c`, and hands the number to
//! this. So the number held here is the one the release gate reads, and xtask
//! spawns nothing for it: no `SPAWNERS` row, and no second gzip whose level or
//! header could differ from the gate's.
//!
//! # Why a tolerance, and why it is measured
//!
//! The job builds with whatever `stable` is that day, and a new compiler can
//! lay the same source out a few bytes differently. A figure held exactly would
//! fail on the toolchain rather than on the code. So each figure carries the
//! spread its profile was measured to show with nothing changed, over runs of
//! the job and under more than one toolchain, with those runs and toolchains
//! written beside it. A tolerance nobody measured is a guess, so a figure
//! without one is refused rather than defaulted to zero: zero is a measurement
//! too, when every build is byte-identical, and then it is written down.
//!
//! # Why it only moves down
//!
//! Growth past the tolerance fails. Growth inside it passes and is not
//! recorded, because recording it would let the figure climb by the noise one
//! run at a time. A shrink passes and prints the `--record` line that takes it,
//! and `--record` refuses a size over the figure: raising a figure is a price,
//! the owner's to accept (H-04), written by hand beside the ruling.
//!
//! # A profile with no figure
//!
//! Fails, so a profile the file lost cannot pass as held. A job whose profile
//! has not been measured yet names it `--unrecorded`, and the size is reported
//! instead. The switch is refused for a profile the file records, so it leaves
//! the job in the commit that records the figure rather than staying behind to
//! excuse the next loss.

use std::collections::BTreeMap;
use std::path::Path;

use crate::json::{self, Json};

/// Where the committed figures live, beside the binding they measure.
pub const FOOTPRINT_PATH: &str = "bindings/js/footprint.json";

// The keys each object may carry. Closed, because `--record` writes a figure
// whole: a key it does not write would be dropped by the next shrink, and a
// key nothing reads is one nobody keeps true.
const FILE_KEYS: &[&str] = &["note", "profiles"];
const PROFILE_KEYS: &[&str] = &["figure", "tolerance"];
const FIGURE_KEYS: &[&str] = &["bytes", "date", "run"];
const TOLERANCE_KEYS: &[&str] = &["bytes", "runs", "toolchains"];

/// One run's measurement of a profile: the size it is held to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Figure {
    /// The gzipped wasm, in bytes, as `gzip -9 -c … | wc -c` counted it.
    pub bytes: u64,
    /// The day it was measured, as `YYYY-MM-DD`.
    pub date: String,
    /// The id of the workflow run that measured it.
    pub run: String,
}

/// How far one revision's size was measured to move with nothing changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tolerance {
    /// The widest spread, in bytes. Zero when every build was byte-identical.
    pub bytes: u64,
    /// The ids of the workflow runs it was measured over.
    pub runs: Vec<String>,
    /// The toolchains those builds were made with.
    pub toolchains: Vec<String>,
}

/// One profile's committed entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    /// What the profile's size is held to.
    pub figure: Figure,
    /// How far over the figure it may measure and still hold.
    pub tolerance: Tolerance,
}

/// The committed file, held to its shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Footprint {
    /// Every recorded profile, by name. A profile not measured yet is absent
    /// rather than listed empty, so "no figure" has one spelling.
    pub profiles: BTreeMap<String, Profile>,
    /// The file as read, which [`record`] writes back with one figure changed.
    json: Json,
}

/// Reads the committed file and holds it to its shape: every profile carries
/// a figure with its bytes, date and run, and a tolerance with its bytes and
/// the runs and toolchains that measured it. Every problem is returned, not
/// only the first.
pub fn parse(text: &str) -> Result<Footprint, Vec<String>> {
    let json = json::parse(text).map_err(|error| vec![format!("is not JSON: {error}")])?;
    let Json::Object(fields) = &json else {
        return Err(vec!["is not a JSON object".to_string()]);
    };
    let mut problems = Vec::new();
    unknown_keys(fields, FILE_KEYS, "the file", &mut problems);
    let mut profiles = BTreeMap::new();
    match fields.get("profiles") {
        Some(Json::Object(entries)) => {
            for (name, entry) in entries {
                if let Some(profile) = read_profile(name, entry, &mut problems) {
                    profiles.insert(name.clone(), profile);
                }
            }
        }
        Some(_) => problems.push("`profiles` is not an object of profiles by name".to_string()),
        None => problems.push(
            "has no `profiles`; a file with nowhere to put a figure holds nothing".to_string(),
        ),
    }
    if problems.is_empty() {
        Ok(Footprint { profiles, json })
    } else {
        Err(problems)
    }
}

fn read_profile(name: &str, entry: &Json, problems: &mut Vec<String>) -> Option<Profile> {
    if name.is_empty() {
        problems.push("a profile has no name".to_string());
        return None;
    }
    let Json::Object(fields) = entry else {
        problems.push(format!("`{name}` is not an object"));
        return None;
    };
    unknown_keys(fields, PROFILE_KEYS, &format!("`{name}`"), problems);
    let figure = match fields.get("figure") {
        Some(figure) => read_figure(name, figure, problems),
        None => {
            problems.push(format!(
                "`{name}` has no figure; a profile not measured yet is left out of \
                 the file, and one it names is held"
            ));
            None
        }
    };
    let tolerance = match fields.get("tolerance") {
        Some(tolerance) => read_tolerance(name, tolerance, problems),
        None => {
            problems.push(format!(
                "`{name}` has no tolerance; held exactly, its figure fails on the \
                 toolchain's drift rather than on the code, and a tolerance is \
                 measured, never defaulted"
            ));
            None
        }
    };
    Some(Profile {
        figure: figure?,
        tolerance: tolerance?,
    })
}

fn read_figure(name: &str, figure: &Json, problems: &mut Vec<String>) -> Option<Figure> {
    let Json::Object(fields) = figure else {
        problems.push(format!("`{name}`'s figure is not an object"));
        return None;
    };
    unknown_keys(fields, FIGURE_KEYS, &format!("`{name}`'s figure"), problems);
    let bytes = match fields.get("bytes").and_then(Json::as_u64) {
        Some(bytes) if bytes > 0 => Some(bytes),
        _ => {
            problems.push(format!("`{name}`'s figure has no whole, non-zero `bytes`"));
            None
        }
    };
    let date = match fields.get("date").and_then(Json::as_str) {
        Some(date) if is_date(date) => Some(date.to_string()),
        _ => {
            problems.push(format!(
                "`{name}`'s figure has no `date` as YYYY-MM-DD; a figure nobody can \
                 date is one nobody can trace"
            ));
            None
        }
    };
    let run = match fields.get("run").and_then(Json::as_str) {
        Some(run) if is_run(run) => Some(run.to_string()),
        _ => {
            problems.push(format!(
                "`{name}`'s figure has no `run`, the id of the workflow run that \
                 measured it"
            ));
            None
        }
    };
    Some(Figure {
        bytes: bytes?,
        date: date?,
        run: run?,
    })
}

fn read_tolerance(name: &str, tolerance: &Json, problems: &mut Vec<String>) -> Option<Tolerance> {
    let Json::Object(fields) = tolerance else {
        problems.push(format!("`{name}`'s tolerance is not an object"));
        return None;
    };
    unknown_keys(
        fields,
        TOLERANCE_KEYS,
        &format!("`{name}`'s tolerance"),
        problems,
    );
    let bytes = fields.get("bytes").and_then(Json::as_u64);
    if bytes.is_none() {
        problems.push(format!("`{name}`'s tolerance has no whole `bytes`"));
    }
    let runs = strings(fields.get("runs"), is_run);
    if runs.is_none() {
        problems.push(format!(
            "`{name}`'s tolerance names no `runs`, the ids of the workflow runs it \
             was measured over; a tolerance without them is a guess"
        ));
    }
    let toolchains = strings(fields.get("toolchains"), |text| !text.trim().is_empty());
    if toolchains.is_none() {
        problems.push(format!(
            "`{name}`'s tolerance names no `toolchains`, the compilers its builds \
             were made with"
        ));
    }
    Some(Tolerance {
        bytes: bytes?,
        runs: runs?,
        toolchains: toolchains?,
    })
}

/// A non-empty array of strings that each pass `keep`, or `None`.
fn strings(value: Option<&Json>, keep: impl Fn(&str) -> bool) -> Option<Vec<String>> {
    let items = value?.as_array()?;
    if items.is_empty() {
        return None;
    }
    items
        .iter()
        .map(|item| item.as_str().filter(|text| keep(text)).map(str::to_string))
        .collect()
}

fn unknown_keys(
    fields: &BTreeMap<String, Json>,
    known: &[&str],
    whose: &str,
    problems: &mut Vec<String>,
) {
    for key in fields.keys() {
        if !known.contains(&key.as_str()) {
            problems.push(format!(
                "{whose} carries `{key}`, which nothing reads; a key nothing reads is \
                 one nobody keeps true"
            ));
        }
    }
}

/// A workflow run's id: GitHub's are decimal.
fn is_run(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// `YYYY-MM-DD`, and a day the calendar has.
fn is_date(text: &str) -> bool {
    let number = |from: usize, to: usize| -> Option<u32> {
        let part = text.get(from..to)?;
        if part.bytes().all(|byte| byte.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    };
    if text.len() != 10 || text.get(4..5) != Some("-") || text.get(7..8) != Some("-") {
        return false;
    }
    let (Some(year), Some(month), Some(day)) = (number(0, 4), number(5, 7), number(8, 10)) else {
        return false;
    };
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// What one `cargo xtask footprint` invocation asks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// The build profile, as the file names it.
    pub profile: String,
    /// Its wasm's gzipped size, as the workflow measured it.
    pub gzipped: u64,
    /// The workflow run that measured it.
    pub run: Option<String>,
    /// The day it was measured, `YYYY-MM-DD`.
    pub date: Option<String>,
    /// A profile with no figure is reported rather than failed.
    pub unrecorded: bool,
    /// Write the size as the profile's figure.
    pub record: bool,
}

impl Options {
    /// Reads the task's options. Every value is held to its form here, so a
    /// size the job failed to measure is refused rather than held as a shrink.
    pub fn parse(args: &[String]) -> Result<Options, String> {
        let mut values: BTreeMap<&str, String> = BTreeMap::new();
        let mut options = Options::default();
        let mut index = 0;
        while index < args.len() {
            let flag = args[index].as_str();
            match flag {
                "--profile" | "--gzipped" | "--run" | "--date" => {
                    index += 1;
                    let value = args
                        .get(index)
                        .ok_or_else(|| format!("`{flag}` needs a value"))?;
                    if values.insert(flag, value.clone()).is_some() {
                        return Err(format!("`{flag}` is given twice"));
                    }
                }
                "--unrecorded" | "--record" => {
                    let switch = if flag == "--record" {
                        &mut options.record
                    } else {
                        &mut options.unrecorded
                    };
                    if *switch {
                        return Err(format!("`{flag}` is given twice"));
                    }
                    *switch = true;
                }
                other => return Err(format!("unknown option `{other}`")),
            }
            index += 1;
        }
        if options.record && options.unrecorded {
            return Err(
                "`--record` and `--unrecorded` disagree: one writes a figure, the other \
                 says there is none"
                    .to_string(),
            );
        }
        options.profile = values
            .remove("--profile")
            .filter(|name| !name.is_empty())
            .ok_or("footprint needs --profile NAME and --gzipped BYTES")?;
        let size = values
            .remove("--gzipped")
            .ok_or("footprint needs --profile NAME and --gzipped BYTES")?;
        // Trimmed, because a `wc -c` that pads its count is still a count.
        let digits = size.trim();
        options.gzipped = match digits.parse::<u64>() {
            Ok(bytes) if bytes > 0 && digits.bytes().all(|byte| byte.is_ascii_digit()) => bytes,
            _ => {
                return Err(format!(
                    "`--gzipped` wants the wasm's gzipped size as a whole number of \
                     bytes, and `{size}` is not one; a size of nothing is a build or a \
                     gzip that failed, not a shrink"
                ))
            }
        };
        if let Some(run) = values.remove("--run") {
            if !is_run(&run) {
                return Err(format!(
                    "`--run` wants the workflow run's id, and `{run}` is not one"
                ));
            }
            options.run = Some(run);
        }
        if let Some(date) = values.remove("--date") {
            if !is_date(&date) {
                return Err(format!(
                    "`--date` wants YYYY-MM-DD, and `{date}` is not one"
                ));
            }
            options.date = Some(date);
        }
        Ok(options)
    }
}

/// Holds one profile's measured size to its committed figure.
///
/// `Ok` is what to print; `Err` is why it fails.
pub fn hold(footprint: &Footprint, options: &Options) -> Result<String, String> {
    let name = &options.profile;
    let now = options.gzipped;
    let Some(profile) = footprint.profiles.get(name) else {
        if options.unrecorded {
            return Ok(format!(
                "`{name}` is {now} bytes gzipped and has no figure; reported, not held, \
                 because the job names it `--unrecorded`"
            ));
        }
        return Err(format!(
            "`{name}` is {now} bytes gzipped and has no figure in {FOOTPRINT_PATH}; a \
             profile the file does not record fails, so one it lost cannot pass as \
             held, unless the job names it `--unrecorded`"
        ));
    };
    let figure = profile.figure.bytes;
    let tolerance = profile.tolerance.bytes;
    if options.unrecorded {
        return Err(format!(
            "`{name}` has a figure of {figure} bytes and the job names it \
             `--unrecorded`; the switch is for a profile with no figure, and left in \
             the job it would let this one pass as unrecorded once the file lost it"
        ));
    }
    let ceiling = figure.saturating_add(tolerance);
    if now > ceiling {
        return Err(format!(
            "`{name}` is {now} bytes gzipped, {} over its figure of {figure} and {} \
             past its tolerance of {tolerance}, the figure run {} measured on {}; \
             growth past the tolerance is a price the owner decides (H-04), not a \
             figure to record",
            now - figure,
            now - ceiling,
            profile.figure.run,
            profile.figure.date
        ));
    }
    if now > figure {
        return Ok(format!(
            "`{name}` is {now} bytes gzipped, {} over its figure of {figure} and inside \
             its tolerance of {tolerance}; held",
            now - figure
        ));
    }
    if now == figure {
        return Ok(format!(
            "`{name}` is {now} bytes gzipped, at its figure; held"
        ));
    }
    Ok(format!(
        "`{name}` is {now} bytes gzipped, {} under its figure of {figure}; held, and \
         this line records the shrink:\n  {}",
        figure - now,
        record_line(options)
    ))
}

/// The line that records a size as its profile's figure.
///
/// Complete when the job passed `--run` and `--date`; otherwise the two are
/// left as places to fill, which `--record` refuses as they stand.
#[must_use]
pub fn record_line(options: &Options) -> String {
    format!(
        "cargo xtask footprint --profile {} --gzipped {} --run {} --date {} --record",
        options.profile,
        options.gzipped,
        options.run.as_deref().unwrap_or("<run id>"),
        options.date.as_deref().unwrap_or("<YYYY-MM-DD>")
    )
}

/// Writes the size as the profile's figure, with the run and the day that
/// measured it, and keeps its tolerance. Returns the file's new text, which
/// has already been read back through [`parse`].
pub fn record(footprint: &Footprint, options: &Options) -> Result<String, String> {
    let name = &options.profile;
    let now = options.gzipped;
    let (Some(run), Some(date)) = (&options.run, &options.date) else {
        return Err(
            "`--record` needs `--run` and `--date`: the figure it writes names the run \
             that measured it and the day"
                .to_string(),
        );
    };
    let Some(profile) = footprint.profiles.get(name) else {
        return Err(format!(
            "`{name}` has no figure and no tolerance to keep; a profile's first figure \
             and its measured tolerance are written by hand from the job's runs, and \
             `--record` only moves a figure whose tolerance it keeps"
        ));
    };
    if now > profile.figure.bytes {
        return Err(format!(
            "`{name}` is {now} bytes gzipped, over its figure of {}; `--record` takes a \
             shrink, and raising a figure is a price the owner decides (H-04)",
            profile.figure.bytes
        ));
    }
    let mut json = footprint.json.clone();
    let entry = match &mut json {
        Json::Object(fields) => match fields.get_mut("profiles") {
            Some(Json::Object(profiles)) => profiles.get_mut(name.as_str()),
            _ => None,
        },
        _ => None,
    };
    let Some(Json::Object(entry)) = entry else {
        return Err(format!("the file as read has no entry for `{name}`"));
    };
    entry.insert(
        "figure".to_string(),
        Json::object([
            ("bytes", Json::count(now)),
            ("date", Json::string(date.clone())),
            ("run", Json::string(run.clone())),
        ]),
    );
    let text = json.to_pretty();
    parse(&text).map_err(|problems| problems.join("; "))?;
    Ok(text)
}

/// What `cargo xtask check` holds: the committed file's shape.
pub fn check(root: &Path) -> Result<(), Vec<String>> {
    let text = std::fs::read_to_string(root.join(FOOTPRINT_PATH))
        .map_err(|error| vec![format!("cannot read {FOOTPRINT_PATH}: {error}")])?;
    parse(&text).map(|_| ()).map_err(|problems| {
        problems
            .into_iter()
            .map(|problem| format!("{FOOTPRINT_PATH}: {problem}"))
            .collect()
    })
}

/// `cargo xtask footprint --profile NAME --gzipped BYTES [options]`.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let options = Options::parse(args)?;
    let path = root.join(FOOTPRINT_PATH);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {FOOTPRINT_PATH}: {error}"))?;
    let footprint =
        parse(&text).map_err(|problems| format!("{FOOTPRINT_PATH}: {}", problems.join("; ")))?;
    if options.record {
        let written = record(&footprint, &options)?;
        std::fs::write(&path, written)
            .map_err(|error| format!("cannot write {FOOTPRINT_PATH}: {error}"))?;
        // No "ok" line here: `main`'s `one` prints one for every task that
        // returns, and two of them read as two checks having passed.
        println!(
            "footprint: recorded `{}` at {} bytes gzipped in {FOOTPRINT_PATH}, its \
             tolerance kept",
            options.profile, options.gzipped
        );
        return Ok(());
    }
    println!("footprint: {}", hold(&footprint, &options)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two recorded profiles, written by hand the way a measured figure is
    /// first written.
    const TWO: &str = r#"{
      "note": ["a note the record keeps"],
      "profiles": {
        "bundled-fonts": {
          "figure": { "bytes": 2000, "date": "2026-10-30", "run": "40000000002" },
          "tolerance": { "bytes": 0, "runs": ["40000000002"],
                         "toolchains": ["rustc 1.99.1"] }
        },
        "default": {
          "figure": { "bytes": 1000, "date": "2026-10-30", "run": "40000000001" },
          "tolerance": { "bytes": 10,
                         "runs": ["40000000001", "40000000003", "40000000004"],
                         "toolchains": ["rustc 1.99.1", "rustc 1.98.0"] }
        }
      }
    }"#;

    fn two() -> Footprint {
        parse(TWO).expect("the fixture holds its shape")
    }

    fn sized(profile: &str, gzipped: u64) -> Options {
        Options {
            profile: profile.to_string(),
            gzipped,
            ..Options::default()
        }
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_string()).collect()
    }

    /// The fixture with the value at `path` replaced, or removed for `None`.
    fn edited(path: &[&str], value: Option<Json>) -> String {
        let mut json = json::parse(TWO).expect("the fixture is JSON");
        let (last, walk) = path.split_last().expect("a path");
        let mut at = &mut json;
        for key in walk {
            let Json::Object(fields) = at else {
                panic!("`{key}` is not under an object")
            };
            at = fields.get_mut(*key).expect("the fixture has the key");
        }
        let Json::Object(fields) = at else {
            panic!("`{last}` is not under an object")
        };
        match value {
            Some(value) => {
                fields.insert((*last).to_string(), value);
            }
            None => {
                fields.remove(*last);
            }
        }
        json.to_pretty()
    }

    /// A repository root holding nothing but `text` as the committed file.
    fn staged(tag: &str, text: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("tinker-footprint-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        let path = root.join(FOOTPRINT_PATH);
        std::fs::create_dir_all(path.parent().expect("the file is in a directory"))
            .expect("a directory");
        std::fs::write(&path, text).expect("a file");
        root
    }

    #[test]
    fn growth_one_byte_past_the_tolerance_fails() {
        let error = hold(&two(), &sized("default", 1011))
            .expect_err("1011 bytes is one past a figure of 1000 and a tolerance of 10");
        assert!(
            error.contains("`default` is 1011 bytes gzipped, 11 over its figure of 1000"),
            "{error}"
        );
        assert!(error.contains("1 past its tolerance of 10"), "{error}");
    }

    #[test]
    fn growth_at_the_tolerance_passes() {
        let said = hold(&two(), &sized("default", 1010))
            .expect("1010 bytes is a figure of 1000 plus its tolerance of 10");
        assert!(
            said.contains("10 over its figure of 1000 and inside its tolerance of 10; held"),
            "{said}"
        );
        // Growth inside the tolerance is the noise, and recording it would let
        // the figure climb by the noise one run at a time.
        assert!(!said.contains("--record"), "{said}");
    }

    #[test]
    fn a_shrink_passes_and_prints_the_new_figure() {
        let options = Options {
            run: Some("40000000009".to_string()),
            date: Some("2026-11-02".to_string()),
            ..sized("default", 990)
        };
        let said = hold(&two(), &options).expect("a shrink passes");
        assert!(said.contains("10 under its figure of 1000"), "{said}");
        assert!(
            said.contains(
                "cargo xtask footprint --profile default --gzipped 990 --run 40000000009 \
                 --date 2026-11-02 --record"
            ),
            "{said}"
        );
    }

    /// Through the task itself, so the file on disk is what is shown to move.
    #[test]
    fn record_writes_the_figure_and_keeps_the_tolerance() {
        let root = staged("record", TWO);
        run(
            &root,
            &args(&[
                "--profile",
                "default",
                "--gzipped",
                "990",
                "--run",
                "40000000009",
                "--date",
                "2026-11-02",
                "--record",
            ]),
        )
        .expect("a shrink records");
        let written = std::fs::read_to_string(root.join(FOOTPRINT_PATH)).expect("the file");
        std::fs::remove_dir_all(&root).ok();
        let before = two();
        let after = parse(&written).expect("what --record writes holds its shape");
        assert_eq!(
            after.profiles.get("default").map(|profile| &profile.figure),
            Some(&Figure {
                bytes: 990,
                date: "2026-11-02".to_string(),
                run: "40000000009".to_string(),
            }),
            "{written}"
        );
        assert_eq!(
            after
                .profiles
                .get("default")
                .map(|profile| &profile.tolerance),
            before
                .profiles
                .get("default")
                .map(|profile| &profile.tolerance),
            "{written}"
        );
        assert_eq!(
            after.profiles.get("bundled-fonts"),
            before.profiles.get("bundled-fonts"),
            "{written}"
        );
        assert!(written.contains("a note the record keeps"), "{written}");
    }

    #[test]
    fn a_profile_without_a_figure_fails_unless_named_unrecorded() {
        let error = hold(&two(), &sized("ocr", 1234))
            .expect_err("a profile the file does not record fails");
        assert!(
            error.contains("`ocr` is 1234 bytes gzipped and has no figure"),
            "{error}"
        );
        assert!(error.contains("--unrecorded"), "{error}");

        let options = Options {
            unrecorded: true,
            ..sized("ocr", 1234)
        };
        let said = hold(&two(), &options).expect("named --unrecorded, it is reported");
        assert!(
            said.contains("`ocr` is 1234 bytes gzipped and has no figure; reported, not held"),
            "{said}"
        );
    }

    #[test]
    fn a_figure_without_a_tolerance_is_refused() {
        let text = edited(&["profiles", "default", "tolerance"], None);
        let problems = parse(&text).expect_err("a figure with no tolerance is refused");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("`default` has no tolerance"),
            "{problems:?}"
        );
    }

    // The neighbours of the six above, which must stay as they are.

    #[test]
    fn a_size_at_its_figure_is_held_and_records_nothing() {
        let said = hold(&two(), &sized("default", 1000)).expect("at the figure holds");
        assert!(said.contains("at its figure; held"), "{said}");
        assert!(!said.contains("--record"), "{said}");
    }

    /// Zero is a measurement, not an absence: a profile measured
    /// byte-identical is held to its figure exactly.
    #[test]
    fn a_tolerance_of_zero_holds_the_figure_exactly() {
        hold(&two(), &sized("bundled-fonts", 2000)).expect("at the figure holds");
        let error = hold(&two(), &sized("bundled-fonts", 2001))
            .expect_err("one byte over a tolerance of zero fails");
        assert!(error.contains("1 past its tolerance of 0"), "{error}");
    }

    /// Run outside the job, a shrink still prints the line, with the run and
    /// the day left as places that `--record` refuses as they stand.
    #[test]
    fn a_shrink_without_its_run_prints_places_for_it() {
        let said = hold(&two(), &sized("default", 990)).expect("a shrink passes");
        assert!(
            said.contains("--gzipped 990 --run <run id> --date <YYYY-MM-DD> --record"),
            "{said}"
        );
        for (option, place) in [("--run", "<run id>"), ("--date", "<YYYY-MM-DD>")] {
            let words = ["--profile", "default", "--gzipped", "990", option, place];
            assert!(Options::parse(&args(&words)).is_err(), "{place}");
        }
    }

    /// The switch is for a profile with no figure; left in the job past the
    /// commit that records one, it would excuse the next loss.
    #[test]
    fn unrecorded_is_refused_for_a_profile_with_a_figure() {
        let options = Options {
            unrecorded: true,
            ..sized("default", 1000)
        };
        let error = hold(&two(), &options).expect_err("the switch outlived its profile");
        assert!(
            error.contains(
                "`default` has a figure of 1000 bytes and the job names it `--unrecorded`"
            ),
            "{error}"
        );
    }

    /// Raising a figure is a price, so `--record` takes a shrink only, inside
    /// the tolerance as well as past it.
    #[test]
    fn record_refuses_growth_even_inside_the_tolerance() {
        for gzipped in [1001, 1010, 1011] {
            let options = Options {
                run: Some("40000000009".to_string()),
                date: Some("2026-11-02".to_string()),
                record: true,
                ..sized("default", gzipped)
            };
            let error = record(&two(), &options).expect_err("growth is not recorded");
            assert!(
                error.contains("over its figure of 1000"),
                "{gzipped}: {error}"
            );
        }
    }

    #[test]
    fn record_needs_the_run_and_its_date() {
        for (run, date) in [
            (None, Some("2026-11-02")),
            (Some("40000000009"), None),
            (None, None),
        ] {
            let options = Options {
                run: run.map(str::to_string),
                date: date.map(str::to_string),
                record: true,
                ..sized("default", 990)
            };
            let error = record(&two(), &options).expect_err("a figure names its run and day");
            assert!(error.contains("needs `--run` and `--date`"), "{error}");
        }
    }

    #[test]
    fn record_refuses_a_profile_with_no_tolerance_to_keep() {
        let options = Options {
            run: Some("40000000009".to_string()),
            date: Some("2026-11-02".to_string()),
            record: true,
            ..sized("ocr", 990)
        };
        let error = record(&two(), &options).expect_err("there is no tolerance to keep");
        assert!(
            error.contains("`ocr` has no figure and no tolerance to keep"),
            "{error}"
        );
    }

    /// Each row changes one thing in the fixture and names the one problem it
    /// must give.
    fn refused_alone<const N: usize>(rows: [([&str; 4], Option<Json>, &str); N]) {
        for (path, value, expected) in rows {
            let path: Vec<&str> = path.into_iter().filter(|key| !key.is_empty()).collect();
            let shown = format!("{path:?} = {value:?}");
            let problems = parse(&edited(&path, value)).expect_err(&shown);
            assert_eq!(problems.len(), 1, "{shown}: {problems:?}");
            assert!(problems[0].contains(expected), "{shown}: {problems:?}");
        }
    }

    #[test]
    fn a_figure_without_its_bytes_date_or_run_is_refused() {
        let at = |key| ["profiles", "default", "figure", key];
        let bytes = "`default`'s figure has no whole, non-zero `bytes`";
        let date = "`default`'s figure has no `date` as YYYY-MM-DD";
        let run = "`default`'s figure has no `run`";
        refused_alone([
            (
                ["profiles", "default", "figure", ""],
                None,
                "`default` has no figure",
            ),
            (at("bytes"), None, bytes),
            (at("bytes"), Some(Json::Number(1000.5)), bytes),
            (at("bytes"), Some(Json::count(0)), bytes),
            (at("bytes"), Some(Json::string("1000")), bytes),
            (at("date"), None, date),
            (at("date"), Some(Json::string("30 October 2026")), date),
            (at("date"), Some(Json::string("2026-02-30")), date),
            (at("run"), None, run),
            (at("run"), Some(Json::string("latest")), run),
            (at("run"), Some(Json::count(40_000_000_001)), run),
        ]);
    }

    #[test]
    fn a_tolerance_without_its_measurement_is_refused() {
        let at = |key| ["profiles", "default", "tolerance", key];
        let bytes = "`default`'s tolerance has no whole `bytes`";
        let runs = "`default`'s tolerance names no `runs`";
        let toolchains = "`default`'s tolerance names no `toolchains`";
        refused_alone([
            (at("bytes"), None, bytes),
            (at("bytes"), Some(Json::Number(10.5)), bytes),
            (at("runs"), None, runs),
            (at("runs"), Some(Json::Array(vec![])), runs),
            (
                at("runs"),
                Some(Json::Array(vec![Json::string("latest")])),
                runs,
            ),
            (at("runs"), Some(Json::string("40000000001")), runs),
            (at("toolchains"), None, toolchains),
            (at("toolchains"), Some(Json::Array(vec![])), toolchains),
            (
                at("toolchains"),
                Some(Json::Array(vec![Json::string(" ")])),
                toolchains,
            ),
        ]);
    }

    #[test]
    fn a_key_the_reader_does_not_know_is_refused() {
        let one = || Some(Json::count(1));
        refused_alone([
            (["schema", "", "", ""], one(), "the file carries `schema`"),
            (
                ["profiles", "default", "budget", ""],
                one(),
                "`default` carries `budget`",
            ),
            (
                ["profiles", "default", "figure", "revision"],
                one(),
                "`default`'s figure carries `revision`",
            ),
            (
                ["profiles", "default", "tolerance", "date"],
                one(),
                "`default`'s tolerance carries `date`",
            ),
        ]);
    }

    #[test]
    fn a_file_without_its_profiles_is_refused() {
        let entry = json::parse(TWO)
            .ok()
            .and_then(|json| json.get("profiles")?.get("default").cloned())
            .expect("the fixture's entry");
        let rows = [
            ("not json".to_string(), "is not JSON"),
            ("[]".to_string(), "is not a JSON object"),
            ("{}".to_string(), "has no `profiles`"),
            (
                r#"{"profiles": []}"#.to_string(),
                "`profiles` is not an object",
            ),
            (
                Json::object([("profiles", Json::object([("", entry)]))]).to_pretty(),
                "a profile has no name",
            ),
            (
                r#"{"profiles": {"default": 1}}"#.to_string(),
                "`default` is not an object",
            ),
        ];
        for (text, expected) in rows {
            let problems = parse(&text).expect_err(&text);
            assert_eq!(problems.len(), 1, "{text}: {problems:?}");
            assert!(problems[0].contains(expected), "{text}: {problems:?}");
        }
    }

    #[test]
    fn the_options_read_as_the_job_writes_them() {
        let options = Options::parse(&args(&[
            "--profile",
            "bundled-fonts",
            "--gzipped",
            "  3412345",
            "--run",
            "40000000009",
            "--date",
            "2028-02-29",
            "--unrecorded",
        ]))
        .expect("they read");
        assert_eq!(
            options,
            Options {
                profile: "bundled-fonts".to_string(),
                gzipped: 3_412_345,
                run: Some("40000000009".to_string()),
                date: Some("2028-02-29".to_string()),
                unrecorded: true,
                record: false,
            }
        );
    }

    /// A job whose build or gzip failed passes nothing, an empty string or a
    /// zero; any of them held as a size would pass as a shrink.
    #[test]
    fn a_size_of_nothing_is_refused() {
        for size in ["0", "", " ", "12k", "-5", "1.5", "+7", "3 412"] {
            let error = Options::parse(&args(&["--profile", "default", "--gzipped", size]))
                .expect_err(size);
            assert!(error.contains("`--gzipped` wants"), "{size:?}: {error}");
        }
    }

    #[test]
    fn options_the_task_cannot_read_are_refused() {
        let rows: &[(&[&str], &str)] = &[
            (
                &["--profile", "default", "--gzipped", "1", "--level", "9"],
                "unknown option `--level`",
            ),
            (
                &["--profile", "default", "--gzipped"],
                "`--gzipped` needs a value",
            ),
            (
                &["--profile", "a", "--profile", "b", "--gzipped", "1"],
                "`--profile` is given twice",
            ),
            (
                &["--profile", "a", "--gzipped", "1", "--record", "--record"],
                "`--record` is given twice",
            ),
            (
                &[
                    "--profile",
                    "a",
                    "--gzipped",
                    "1",
                    "--record",
                    "--unrecorded",
                ],
                "disagree",
            ),
            (&["--gzipped", "1"], "needs --profile NAME"),
            (&["--profile", "", "--gzipped", "1"], "needs --profile NAME"),
            (
                &["--profile", "a"],
                "needs --profile NAME and --gzipped BYTES",
            ),
            (
                &["--profile", "a", "--gzipped", "1", "--run", "latest"],
                "`--run` wants",
            ),
            (
                &["--profile", "a", "--gzipped", "1", "--date", "2026-13-01"],
                "`--date` wants",
            ),
            (
                &["--profile", "a", "--gzipped", "1", "--date", "2027-02-29"],
                "`--date` wants",
            ),
        ];
        for (words, expected) in rows {
            let error = Options::parse(&args(words)).expect_err(&format!("{words:?}"));
            assert!(error.contains(expected), "{words:?}: {error}");
        }
    }

    #[test]
    fn the_committed_file_holds_its_shape() {
        if let Err(problems) = check(&crate::repo_root()) {
            panic!("{FOOTPRINT_PATH}:\n{}", problems.join("\n"));
        }
    }
}
