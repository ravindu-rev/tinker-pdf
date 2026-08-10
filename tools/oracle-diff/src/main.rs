//! `oracle-diff` — compares this engine against an external one.
//!
//! The oracles are invoked as subprocesses and never linked, which is what
//! keeps their licences out of this tree entirely: their output is used as a
//! comparison reference during development and is never redistributed.
//!
//! An oracle is not an authority. Two of the defects that motivated this
//! engine are cases where the oracle is the one that is wrong, so a
//! disagreement is a thing to look at rather than a failure to fix. What this
//! reports is *where* they differ and by how much; deciding who is right is a
//! person's job.
//!
//! Every oracle is optional. A missing one is reported and skipped, because
//! requiring all of them would mean nobody can run this.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
oracle-diff — compare tinker-pdf against an external renderer or extractor

usage:
  oracle-diff render <file.pdf>... [--oracle NAME] [--dpi D] [--budget F] [--keep DIR]
  oracle-diff text   <file.pdf>... [--oracle NAME]
  oracle-diff which

oracles:
  mutool      `mutool draw` / `mutool draw -F txt`
  pdftoppm    `pdftoppm` (poppler); text uses `pdftotext`
  pdfium      `pdfium_test --ppm`

options:
  --oracle NAME  which oracle to use; the default is the first one found
  --dpi D        resolution for a render comparison (default 150)
  --budget F     the largest acceptable mean difference (default 0.02)
  --keep DIR     keep the rendered images here instead of a temporary directory

`which` reports which oracles are installed and exits. A missing oracle is
never an error: it is reported and skipped, because requiring every oracle
would mean nobody can run this at all.

The budget defaults far looser than pdfcmp's because a different engine is
being compared, not a different build of this one. Anti-aliasing, hinting and
colour conversion all differ legitimately.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("oracle-diff: {message}");
            ExitCode::from(2)
        }
    }
}

/// One external program this knows how to drive.
struct Oracle {
    name: &'static str,
    program: &'static str,
    /// The arguments that render a page to an image file.
    render: fn(&str, &str, f64) -> Vec<String>,
    /// The arguments that extract text, and the program that does it, which is
    /// not always the same one.
    text_program: &'static str,
    text: fn(&str) -> Vec<String>,
}

const ORACLES: &[Oracle] = &[
    Oracle {
        name: "mutool",
        program: "mutool",
        render: |input, output, dpi| {
            vec![
                "draw".into(),
                "-r".into(),
                format!("{dpi}"),
                "-o".into(),
                output.into(),
                input.into(),
                "1".into(),
            ]
        },
        text_program: "mutool",
        text: |input| {
            vec![
                "draw".into(),
                "-F".into(),
                "txt".into(),
                "-o".into(),
                "-".into(),
                input.into(),
            ]
        },
    },
    Oracle {
        name: "pdftoppm",
        program: "pdftoppm",
        render: |input, output, dpi| {
            // pdftoppm appends its own suffix, so the caller strips it back
            // off when it looks for the result.
            vec![
                "-r".into(),
                format!("{dpi}"),
                "-f".into(),
                "1".into(),
                "-l".into(),
                "1".into(),
                "-ppm".into(),
                input.into(),
                output.trim_end_matches(".ppm").into(),
            ]
        },
        text_program: "pdftotext",
        text: |input| vec![input.into(), "-".into()],
    },
    Oracle {
        name: "pdfium",
        program: "pdfium_test",
        render: |input, output, _dpi| {
            vec![
                "--ppm".into(),
                format!("--pages=0"),
                format!("--output={output}"),
                input.into(),
            ]
        },
        text_program: "pdfium_test",
        text: |input| vec!["--show-text".into(), input.into()],
    },
];

/// Whether a program can be run at all.
///
/// Asking it for its version is the portable way: `which` is not on Windows
/// and `where` is not on Unix, and both lie about shell builtins.
fn available(program: &str) -> bool {
    Command::new(program)
        .arg("-v")
        .output()
        .or_else(|_| Command::new(program).arg("--version").output())
        .is_ok()
}

fn pick(requested: Option<&str>) -> Result<&'static Oracle, String> {
    match requested {
        Some(name) => {
            let oracle = ORACLES
                .iter()
                .find(|o| o.name == name)
                .ok_or_else(|| format!("no oracle called `{name}`"))?;
            if !available(oracle.program) {
                return Err(format!("`{}` is not installed", oracle.program));
            }
            Ok(oracle)
        }
        None => ORACLES
            .iter()
            .find(|o| available(o.program))
            .ok_or_else(|| "no oracle is installed; try `oracle-diff which`".to_string()),
    }
}

fn run(args: &[String]) -> Result<bool, String> {
    let command = args[0].as_str();
    let mut files = Vec::new();
    let mut oracle_name = None;
    let mut dpi = 150.0f64;
    let mut budget = 0.02f64;
    let mut keep: Option<String> = None;

    let mut index = 1;
    while index < args.len() {
        let arg = args[index].as_str();
        let mut value = || -> Result<String, String> {
            index += 1;
            args.get(index)
                .cloned()
                .ok_or_else(|| format!("`{arg}` needs a value"))
        };
        match arg {
            "--oracle" => oracle_name = Some(value()?),
            "--dpi" => {
                dpi = value()?
                    .parse()
                    .map_err(|_| "--dpi takes a number".to_string())?;
            }
            "--budget" => {
                budget = value()?
                    .parse()
                    .map_err(|_| "--budget takes a number".to_string())?;
            }
            "--keep" => keep = Some(value()?),
            _ if arg.starts_with("--") => return Err(format!("unknown option `{arg}`")),
            _ => files.push(arg.to_string()),
        }
        index += 1;
    }

    if command == "which" {
        for oracle in ORACLES {
            println!(
                "{:<10} {}",
                oracle.name,
                if available(oracle.program) {
                    "installed"
                } else {
                    "missing"
                }
            );
        }
        return Ok(true);
    }

    if files.is_empty() {
        return Err("no input file".to_string());
    }
    let oracle = pick(oracle_name.as_deref())?;
    eprintln!("using {}", oracle.name);

    let workspace = match &keep {
        Some(dir) => {
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {dir}: {e}"))?;
            PathBuf::from(dir)
        }
        None => {
            let dir = std::env::temp_dir().join("oracle-diff");
            std::fs::create_dir_all(&dir).map_err(|e| format!("creating a workspace: {e}"))?;
            dir
        }
    };

    let mut disagreed = 0usize;
    for file in &files {
        let outcome = match command {
            "render" => compare_render(oracle, file, &workspace, dpi, budget),
            "text" => compare_text(oracle, file),
            other => return Err(format!("unknown command `{other}`")),
        };
        match outcome {
            Ok(true) => {}
            Ok(false) => disagreed += 1,
            Err(message) => {
                eprintln!("oracle-diff: {file}: {message}");
                disagreed += 1;
            }
        }
    }

    println!("{} files, {disagreed} disagreed", files.len());
    Ok(disagreed == 0)
}

fn compare_render(
    oracle: &Oracle,
    file: &str,
    workspace: &Path,
    dpi: f64,
    budget: f64,
) -> Result<bool, String> {
    let stem = Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "page".to_string());

    let theirs = workspace.join(format!("{stem}-oracle.ppm"));
    let theirs_str = theirs.to_string_lossy().into_owned();

    let arguments = (oracle.render)(file, &theirs_str, dpi);
    let output = Command::new(oracle.program)
        .args(&arguments)
        .output()
        .map_err(|e| format!("running {}: {e}", oracle.program))?;
    if !output.status.success() {
        return Err(format!(
            "{} failed: {}",
            oracle.program,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    // pdftoppm decides its own filename, so the one that appeared has to be
    // found rather than assumed.
    let produced = if theirs.exists() {
        theirs
    } else {
        find_produced(workspace, &stem)
            .ok_or_else(|| format!("{} did not write an image anyone can find", oracle.program))?
    };

    // The comparison itself is pdfcmp's job, invoked the same way a person
    // would — one metric, defined in one place.
    let pdfcmp = std::env::var("PDFCMP").unwrap_or_else(|_| "pdfcmp".to_string());
    let result = Command::new(&pdfcmp)
        .arg(produced)
        .arg(file)
        .arg("--dpi")
        .arg(format!("{dpi}"))
        .arg("--budget")
        .arg(format!("{budget}"))
        .output()
        .map_err(|e| format!("running {pdfcmp}: {e}; set PDFCMP to its path"))?;

    print!("{file}: {}", String::from_utf8_lossy(&result.stdout));
    if !result.status.success() {
        eprint!("{}", String::from_utf8_lossy(&result.stderr));
    }
    Ok(result.status.success())
}

fn find_produced(workspace: &Path, stem: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(workspace).ok()?;
    entries.flatten().map(|e| e.path()).find(|p| {
        p.file_name()
            .map(|n| n.to_string_lossy().starts_with(stem))
            .unwrap_or(false)
            && p.extension().is_some_and(|e| e == "ppm" || e == "pnm")
    })
}

fn compare_text(oracle: &Oracle, file: &str) -> Result<bool, String> {
    let arguments = (oracle.text)(file);
    let output = Command::new(oracle.text_program)
        .args(&arguments)
        .output()
        .map_err(|e| format!("running {}: {e}", oracle.text_program))?;

    let theirs = normalize(&String::from_utf8_lossy(&output.stdout));

    let bytes = std::fs::read(file).map_err(|e| format!("reading {file}: {e}"))?;
    let doc = tinker_pdf::Document::open(bytes).map_err(|e| format!("{e:?}"))?;
    let ours = normalize(
        &doc.pages()
            .iter()
            .map(|p| p.text().plain_text())
            .collect::<Vec<_>>()
            .join("\n"),
    );

    // Similarity by shared words rather than by exact match. Extractors
    // legitimately differ on line breaks, hyphenation and the order of
    // absolutely positioned fragments, and an exact comparison would report
    // every file as different while saying nothing about which are wrong.
    let similarity = similarity(&ours, &theirs);
    println!(
        "{file}: {:.1}% similar to {}",
        similarity * 100.0,
        oracle.name
    );

    if similarity < 0.98 {
        let (only_ours, only_theirs) = differing_words(&ours, &theirs);
        if !only_ours.is_empty() {
            println!("  only ours:   {}", preview(&only_ours));
        }
        if !only_theirs.is_empty() {
            println!("  only theirs: {}", preview(&only_theirs));
        }
        return Ok(false);
    }
    Ok(true)
}

fn normalize(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// The fraction of words the two extractions agree on, counting repeats.
fn similarity(ours: &[String], theirs: &[String]) -> f64 {
    if ours.is_empty() && theirs.is_empty() {
        return 1.0;
    }
    let mut counts: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for word in ours {
        *counts.entry(word.as_str()).or_default() += 1;
    }

    let mut shared = 0i64;
    for word in theirs {
        if let Some(count) = counts.get_mut(word.as_str()) {
            if *count > 0 {
                *count -= 1;
                shared += 1;
            }
        }
    }

    let total = ours.len().max(theirs.len()) as f64;
    shared as f64 / total
}

fn differing_words(ours: &[String], theirs: &[String]) -> (Vec<String>, Vec<String>) {
    let mine: std::collections::HashSet<&str> = ours.iter().map(String::as_str).collect();
    let yours: std::collections::HashSet<&str> = theirs.iter().map(String::as_str).collect();

    let mut only_ours: Vec<String> = mine.difference(&yours).map(|s| (*s).to_string()).collect();
    let mut only_theirs: Vec<String> = yours.difference(&mine).map(|s| (*s).to_string()).collect();
    only_ours.sort();
    only_theirs.sort();
    (only_ours, only_theirs)
}

fn preview(words: &[String]) -> String {
    let shown: Vec<&str> = words.iter().take(12).map(String::as_str).collect();
    if words.len() > shown.len() {
        format!(
            "{} ... and {} more",
            shown.join(" "),
            words.len() - shown.len()
        )
    } else {
        shown.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_extractions_are_completely_similar() {
        let a = normalize("The quick brown fox.");
        assert_eq!(similarity(&a, &a), 1.0);
    }

    /// Punctuation, case and whitespace differ between extractors for reasons
    /// that have nothing to do with correctness.
    #[test]
    fn formatting_differences_do_not_count_as_disagreement() {
        let ours = normalize("The quick\nbrown fox");
        let theirs = normalize("the  QUICK brown, fox!");
        assert_eq!(similarity(&ours, &theirs), 1.0);
    }

    #[test]
    fn a_missing_word_shows_up_as_a_difference() {
        let ours = normalize("one two three four");
        let theirs = normalize("one two three");
        assert!(similarity(&ours, &theirs) < 1.0);

        let (only_ours, only_theirs) = differing_words(&ours, &theirs);
        assert_eq!(only_ours, vec!["four"]);
        assert!(only_theirs.is_empty());
    }

    /// Two empty extractions agree, and must not divide by zero doing so.
    #[test]
    fn two_empty_extractions_agree() {
        assert_eq!(similarity(&[], &[]), 1.0);
        assert_eq!(similarity(&normalize("a"), &[]), 0.0);
    }

    /// A word repeated more often on one side is a real difference, so the
    /// count matters rather than only the set.
    #[test]
    fn repeated_words_are_counted_not_deduplicated() {
        let ours = normalize("a a a");
        let theirs = normalize("a");
        assert!((similarity(&ours, &theirs) - 1.0 / 3.0).abs() < 1e-9);
    }
}
