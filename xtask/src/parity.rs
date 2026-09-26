//! `cargo xtask bindings-parity` — the four write surfaces, compared to one
//! recorded answer (gap 32 milestone 6).
//!
//! Ruling 11 says a binding projects the facade 1:1 and adds no logic of its
//! own. That is a claim, and this is the check: two scripts with every input
//! pinned, run through the facade, the wheel, the npm package and the NuGet
//! package, must produce **byte-identical** output. Four surfaces disagreeing
//! means one of them added something.
//!
//! **Two failures this is built to catch, and they are different.**
//!
//! - A *mismatch*: a surface printed a hash and it is not the recorded one.
//!   That is the one everybody thinks of.
//! - An *absent line*: a surface ran, exited zero, and printed no
//!   `WROTE sha256=` at all. That is the one that gets shipped, because a
//!   script that silently does nothing looks exactly like a passing one from
//!   the outside. Both exit non-zero here.
//!
//! And a third thing, which is not a failure and must not be silence: a
//! surface whose artefact is not installed on this machine is **SKIPPED**, and
//! says so by name with the reason. Retired ruling 9 left that discipline
//! behind it and ruling 13 restates it — a check that can be absent announces
//! whether it ran. `--require-all` turns every skip into a failure, which is
//! what CI passes.
//!
//! **Agreement is not enough, and the surfaces know it.** Four byte-identical
//! outputs tell you nothing if all four are wrong, so every surface re-opens
//! its own artefact through this engine's strict structural validator before
//! it prints a hash, and refuses to print one if the artefact is not clean.
//! Under ruling 13 that validator is first-party, which is exactly why it can
//! be relied on here instead of being an external step somebody might not have
//! installed. A `WROTE` line is therefore already a statement that the bytes
//! are a valid document; this program's job is that the four agree about
//! *which* one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The recorded answer, in one place.
///
/// It lives here rather than beside the three synthesised-document hashes in
/// `crates/tinker-pdf/tests/determinism.rs` for a practical reason: those are
/// a *rendering* claim's fingerprints, changed only by the renderer or the
/// synthesiser, and interleaving a bindings hash with them would make a
/// writer change look like a determinism regression to whoever reads that file
/// next.
///
/// One place, and four surfaces compared to *it* rather than to each other:
/// a legitimate writer change is then one recorded update here, not four
/// flaky suites. If these move, the reason belongs in the commit message —
/// a hash that moves without one is indistinguishable from a hash that broke.
///
/// Recorded August 2026 on windows/x86_64, and target-independent by ruling 4:
/// the writer's output is fixed-point and integer throughout, and the one
/// input that would otherwise vary — encryption entropy — is not used by
/// either script.
const EXPECTED: &[(&str, &str)] = &[
    (
        "fill-and-save",
        "59f1efce6e4e5bfa8915fdee31e43f629e6373512de8040bf8e6404b7fe78af3",
    ),
    (
        "build-a-document",
        "1dbb7ace2a5787016efa257ab8c3efdb6ceae1b339f8597266ad47c5828dac62",
    ),
];

/// What one surface's run came to.
enum Outcome {
    /// It ran and every hash it printed agreed.
    Ran,
    /// It could not be attempted, and this is why. Never silent.
    Skipped(String),
    /// It ran and something was wrong.
    Failed(Vec<String>),
}

/// One surface: how to run it, and what to call it.
struct Surface {
    /// The `surface=` value its script prints, which is also its name here.
    name: &'static str,
    /// Why it was skipped, or the command to run.
    plan: Result<Command, String>,
}

/// `cargo xtask bindings-parity`.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut require_all = false;
    let mut node_dir: Option<PathBuf> = None;
    let mut python = "python".to_string();

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--require-all" => require_all = true,
            "--node-dir" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    return Err("--node-dir needs a directory".to_string());
                };
                node_dir = Some(PathBuf::from(value));
            }
            "--python" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    return Err("--python needs an interpreter".to_string());
                };
                python = value.clone();
            }
            other => return Err(format!("unknown option `{other}`")),
        }
        index += 1;
    }

    let fixture = root.join("testdata/form-fields.pdf");
    if !fixture.is_file() {
        return Err(format!(
            "{} is missing, and it is the input both scripts open",
            fixture.display()
        ));
    }

    let surfaces = vec![
        facade(root, &fixture),
        python_surface(root, &fixture, &python),
        js_surface(root, &fixture, node_dir.as_deref()),
        dotnet_surface(root, &fixture),
    ];

    let mut ran = Vec::new();
    let mut skipped = Vec::new();
    let mut problems = Vec::new();

    for surface in surfaces {
        match evaluate(surface) {
            (name, Outcome::Ran) => {
                println!("bindings-parity: {name} RAN, both hashes agree");
                ran.push(name);
            }
            (name, Outcome::Skipped(why)) => {
                println!("bindings-parity: {name} SKIPPED — {why}");
                skipped.push((name, why));
            }
            (name, Outcome::Failed(found)) => {
                for problem in found {
                    eprintln!("bindings-parity: {name}: {problem}");
                }
                problems.push(name);
            }
        }
    }

    println!(
        "bindings-parity: RAN {} ({}), SKIPPED {} ({})",
        ran.len(),
        if ran.is_empty() {
            "none".to_string()
        } else {
            ran.join(", ")
        },
        skipped.len(),
        if skipped.is_empty() {
            "none".to_string()
        } else {
            skipped
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(", ")
        }
    );

    if !problems.is_empty() {
        return Err(format!(
            "{} surface(s) disagreed or produced no evidence: {}",
            problems.len(),
            problems.join(", ")
        ));
    }

    // The facade surface is `cargo run`, so it cannot legitimately be absent:
    // this program is itself running under cargo. A skip there means the
    // example failed to build or the checkout is broken, and passing on it
    // would be the silent-pass this whole task exists against.
    if !ran.contains(&"facade") {
        return Err(
            "the facade surface did not run, and it is the one every other \
             surface is compared against"
                .to_string(),
        );
    }

    if require_all && !skipped.is_empty() {
        return Err(format!(
            "--require-all, and {} surface(s) were skipped: {}",
            skipped.len(),
            skipped
                .iter()
                .map(|(name, why)| format!("{name} ({why})"))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    if skipped.is_empty() {
        println!("bindings-parity: all four surfaces wrote the same bytes");
    } else {
        println!(
            "bindings-parity: the surfaces that ran agree; {} did not run and \
             this run therefore proves nothing about them",
            skipped.len()
        );
    }
    Ok(())
}

/// Runs one surface and judges what it printed.
fn evaluate(surface: Surface) -> (&'static str, Outcome) {
    let mut command = match surface.plan {
        Ok(command) => command,
        Err(why) => return (surface.name, Outcome::Skipped(why)),
    };

    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            return (
                surface.name,
                Outcome::Failed(vec![format!("could not be run: {error}")]),
            )
        }
    };

    let mut problems = Vec::new();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        problems.push(format!(
            "exited {}; stderr: {}",
            output.status,
            stderr.trim().lines().last().unwrap_or("(empty)")
        ));
    }

    let found = harvest(&stdout);
    for (script, expected) in EXPECTED {
        match found.get(*script) {
            // The failure that gets shipped: a script that ran, exited zero
            // and produced no evidence looks exactly like a passing one.
            None => problems.push(format!(
                "printed no `WROTE sha256=` line for {script}, which is not the \
                 same as printing a wrong one — a surface that silently writes \
                 nothing passes every check that only compares what it printed"
            )),
            Some(actual) if actual != expected => problems.push(format!(
                "{script}: wrote {actual}, and the recorded answer is {expected}"
            )),
            Some(_) => {}
        }
    }

    for script in found.keys() {
        if !EXPECTED.iter().any(|(known, _)| known == script) {
            problems.push(format!(
                "printed a hash for {script}, which is not a script this \
                 compares — either the script list moved or the surface is \
                 running something else"
            ));
        }
    }

    if problems.is_empty() {
        (surface.name, Outcome::Ran)
    } else {
        (surface.name, Outcome::Failed(problems))
    }
}

/// Every `WROTE sha256=<hex> ... script=<name>` line, as script to hash.
///
/// Parsed by key rather than by position so a surface may print whatever else
/// it likes around them — the .NET smoke prefixes its own name, the JavaScript
/// one reports its transaction leg — without this having to know about it.
fn harvest(stdout: &str) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    for line in stdout.lines() {
        let Some(start) = line.find("WROTE sha256=") else {
            continue;
        };
        let rest = &line[start..];
        let mut hash = None;
        let mut script = None;
        for field in rest.split_whitespace() {
            if let Some(value) = field.strip_prefix("sha256=") {
                hash = Some(value.to_string());
            }
            if let Some(value) = field.strip_prefix("script=") {
                script = Some(value.to_string());
            }
        }
        if let (Some(hash), Some(script)) = (hash, script) {
            found.insert(script, hash);
        }
    }
    found
}

/// The reference surface: the facade itself, through a cargo example.
fn facade(root: &Path, fixture: &Path) -> Surface {
    let mut command = Command::new(env!("CARGO"));
    command
        .current_dir(root)
        .args([
            "run",
            "--quiet",
            "-p",
            "tinker-pdf",
            "--example",
            "write_parity",
            "--",
        ])
        .arg(fixture);
    Surface {
        name: "facade",
        plan: Ok(command),
    }
}

/// The wheel, through whichever interpreter has it installed.
///
/// Detected by *importing* rather than by finding an interpreter: a Python
/// that exists and has no `tinker_pdf` in it would otherwise be reported as a
/// failure, when the honest answer is that the wheel is not installed here.
fn python_surface(root: &Path, fixture: &Path, python: &str) -> Surface {
    let importable = Command::new(python)
        .args(["-c", "import tinker_pdf"])
        .current_dir(root)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);

    if !importable {
        return Surface {
            name: "python",
            plan: Err(format!(
                "`{python} -c \"import tinker_pdf\"` failed, so no wheel is \
                 installed for this interpreter; `maturin build` and `pip \
                 install` it, or pass --python"
            )),
        };
    }

    let mut command = Command::new(python);
    command
        .current_dir(root)
        .arg(root.join("bindings/python/tests/write_parity.py"))
        .arg(fixture);
    Surface {
        name: "python",
        plan: Ok(command),
    }
}

/// The npm package, from a directory where it has been installed.
///
/// The directory is the whole point: resolving by package name from a cwd is
/// what makes this a test of the install rather than of `bindings/js/pkg`,
/// which is a build product.
fn js_surface(root: &Path, fixture: &Path, node_dir: Option<&Path>) -> Surface {
    let default = root.join("target/js-parity");
    let dir = node_dir.map_or(default, Path::to_path_buf);
    if !dir.join("node_modules/tinker-pdf-js").is_dir() {
        return Surface {
            name: "js",
            plan: Err(format!(
                "{} holds no installed tinker-pdf-js; `wasm-pack build --target \
                 web`, `npm pack` and `npm install` the tarball there, or pass \
                 --node-dir",
                dir.display()
            )),
        };
    }

    let mut command = Command::new("node");
    command
        .current_dir(&dir)
        .arg(root.join("bindings/js/tests/write_parity.mjs"))
        .arg(fixture);
    Surface {
        name: "js",
        plan: Ok(command),
    }
}

/// The NuGet package, through the smoke project that consumes it.
///
/// The smoke needs a face as well as a document, because its read leg asserts
/// blank-then-inked; the write leg is the third argument. Without a face this
/// is skipped rather than run with a substitute, since a substituted input is
/// a different test wearing the same name.
fn dotnet_surface(root: &Path, fixture: &Path) -> Surface {
    let package = root.join("target/nuget");
    if !package.is_dir() {
        return Surface {
            name: "dotnet",
            plan: Err(format!(
                "{} holds no packed TinkerPdf; `cargo xtask nuget-stage` then \
                 `dotnet pack bindings/dotnet/TinkerPdf.csproj -c Release -o \
                 target/nuget`",
                package.display()
            )),
        };
    }
    let Some(face) = system_face() else {
        return Surface {
            name: "dotnet",
            plan: Err(
                "no system face was found, and the smoke's read leg asserts \
                 blank-then-inked before its write leg runs"
                    .to_string(),
            ),
        };
    };

    let mut command = Command::new("dotnet");
    command
        .current_dir(root)
        .args([
            "run",
            "--project",
            "bindings/dotnet/tests/Smoke",
            "-c",
            "Release",
            "--",
        ])
        .arg(root.join("testdata/simple-text.pdf"))
        .arg(face)
        .arg(fixture);
    Surface {
        name: "dotnet",
        plan: Ok(command),
    }
}

/// One face per platform the release matrix builds on.
///
/// The same list `wheel_smoke.py` carries, and a list rather than a fallback
/// to "render without a face" for the reason stated there: a missing face
/// would otherwise turn an inked assertion into a skip, and a skip exits zero
/// and reads exactly like a pass.
fn system_face() -> Option<PathBuf> {
    const FACES: &[&str] = &[
        "C:/Windows/Fonts/arial.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/Library/Fonts/Arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    ];
    FACES.iter().map(PathBuf::from).find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parser reads the fields by name, so a surface may print whatever
    /// else it likes on the line and around it.
    #[test]
    fn hashes_are_harvested_by_key_and_not_by_position() {
        let stdout = "\
engine version 0.0.1
DOTNET-SMOKE: WROTE sha256=aaaa surface=dotnet script=fill-and-save bytes=10
WROTE sha256=bbbb surface=facade script=build-a-document bytes=20
DOTNET-PARITY: RAN
";
        let found = harvest(stdout);
        assert_eq!(found.get("fill-and-save").map(String::as_str), Some("aaaa"));
        assert_eq!(
            found.get("build-a-document").map(String::as_str),
            Some("bbbb")
        );
        assert_eq!(found.len(), 2);
    }

    /// A line without a `script=` is not a hash about anything, so it is not
    /// counted — otherwise a malformed line would satisfy the presence check
    /// for whichever script happened to be missing.
    #[test]
    fn a_line_with_no_script_is_not_evidence() {
        assert!(harvest("WROTE sha256=aaaa surface=facade\n").is_empty());
        assert!(harvest("nothing here at all\n").is_empty());
    }

    /// The two scripts are named once and the names are what every surface
    /// prints, so a rename that reached only one of them is a mismatch rather
    /// than a silent pass.
    #[test]
    fn the_recorded_answer_names_both_scripts_once() {
        assert_eq!(EXPECTED.len(), 2);
        let names: Vec<&str> = EXPECTED.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["fill-and-save", "build-a-document"]);
        for (name, hash) in EXPECTED {
            assert_eq!(hash.len(), 64, "{name}: a SHA-256 is 64 hex characters");
            assert!(
                hash.chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
                "{name}: lower-case hex, which is what every surface prints"
            );
        }
    }
}
