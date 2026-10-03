//! `cargo xtask bindings-parity` — every binding surface, compared to one
//! recorded answer (gap 32 milestone 6).
//!
//! Ruling 11 says a binding projects the facade 1:1 and adds no logic of its
//! own. That is a claim, and this is the check: scripts with every input
//! pinned, run through the facade, the wheel, the npm package, the NuGet
//! package and the Go, Ruby and Java bindings over the C ABI, must produce
//! **byte-identical** output. Eight of them write a document and print
//! `WROTE sha256=` of its bytes; `sanitise-report`, `read-surface`,
//! `signatures` and `form-data` write down what a sanitise reported,
//! everything the read surface says about documents, and what form data says,
//! in a text whose every byte is specified
//! (`crates/tinker-pdf/examples/write_parity.rs`) and print `READ sha256=` of
//! that. Surfaces disagreeing means one of them added something — or, on the
//! read side, dropped or reordered something.
//!
//! **Two failures this is built to catch, and they are different.**
//!
//! - A *mismatch*: a surface printed a hash and it is not the recorded one.
//!   That is the one everybody thinks of.
//! - An *absent line*: a surface ran, exited zero, and printed no
//!   `WROTE sha256=` or `READ sha256=` for a script at all. That is the one
//!   that gets shipped, because a script that silently does nothing looks
//!   exactly like a passing one from the outside. Both exit non-zero here.
//!
//! And a third thing, which is not a failure and must not be silence: a
//! surface whose artefact is not installed on this machine is **SKIPPED**, and
//! says so by name with the reason. Retired ruling 9 left that discipline
//! behind it and ruling 13 restates it — a check that can be absent announces
//! whether it ran. `--require-all` turns every skip into a failure, which is
//! what CI passes.
//!
//! **Agreement is not enough, and the surfaces know it.** Four byte-identical
//! outputs tell you nothing if all of them are wrong, so every surface re-opens
//! its own artefact through this engine's strict structural validator before
//! it prints a hash, and refuses to print one if the artefact is not clean.
//! Under ruling 13 that validator is first-party, which is exactly why it can
//! be relied on here instead of being an external step somebody might not have
//! installed. A `WROTE` line is therefore already a statement that the bytes
//! are a valid document; this program's job is that the surfaces agree about
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
/// One place, and every surface compared to *it* rather than to each other:
/// a legitimate writer change is then one recorded update here, not a flaky
/// suite per language. If these move, the reason belongs in the commit
/// message — a hash that moves without one is indistinguishable from a hash
/// that broke.
///
/// The two write hashes were recorded August 2026 on windows/x86_64, and are
/// target-independent by ruling 4: the writer's output is fixed-point and
/// integer throughout, and the one input that would otherwise vary —
/// encryption entropy — is not used by either script; nor by `document-ops`
/// and `sanitise`, recorded October 2026 on linux/x86_64 with the read hashes.
/// `read-surface` moved when it gained the document-ops artefact and every
/// page's five boundaries, from be7deb04... to c02151fc.... The read texts
/// spell every number by its IEEE bits or as an integer, so they are as
/// target-independent as the reads under them. `signatures` judges validity
/// only at the instant it names, never at "now", so it does not move with the
/// calendar. It moved from e2f5e33c... to c3490eb5... when it gained
/// `ecdsa-p256-altered`, the one verdict whose digest and signature check
/// disagree: before it, a surface reading either answer from the other's
/// accessor agreed with every other surface, which the Ruby binding's
/// injection campaign showed. `forms` and `form-data` were recorded October
/// 2026 on linux/x86_64 when the forms surface crossed; the form-data text
/// hashes the FDF and XFDF the writer makes, both of which are text with no
/// date or identifier in them, so they are as fixed as the rest. `graphics`
/// was recorded the same way when the builder's graphics resources crossed.
const EXPECTED: &[(&str, &str)] = &[
    (
        "fill-and-save",
        "59f1efce6e4e5bfa8915fdee31e43f629e6373512de8040bf8e6404b7fe78af3",
    ),
    (
        "build-a-document",
        "1dbb7ace2a5787016efa257ab8c3efdb6ceae1b339f8597266ad47c5828dac62",
    ),
    (
        "document-ops",
        "a8c436d092a929ce9ddfbc53b5fe1f48f7ff2141d23148a7b1e003a4dfb445e9",
    ),
    (
        "sanitise",
        "f6f9cedc25ac039b7b45d4baf8507ca38610228a8c3867ed3898b72ec239451d",
    ),
    (
        "save-options",
        "652c7cd32149a0f6fd06e921fa9762e2c8411aa09fbfc732ea6a2c991e369704",
    ),
    (
        "save-linearized",
        "e64bffa59ffbc7a4b7335abdc634bc567a615d9f29e23ef1673c51e07f3ac7fc",
    ),
    (
        "sanitise-report",
        "a73b92e55800b856cb0f46107d89ce26d51ca9f8c941e536790305255d26300f",
    ),
    (
        "read-surface",
        "c02151fc924133fe2864d1b05bc57e5eaafaddab86f0be2b2088549fd91af5c0",
    ),
    (
        "signatures",
        "c3490eb5f9a5c893893494bf0c51269ef718f915051053d6b30ff7ae7c1ad2ff",
    ),
    (
        "forms",
        "84b2daef81a1d6342fec8052971b25ea6ab82a366cd3afcd068c490806f1bc3b",
    ),
    (
        "form-data",
        "f81ce8279205bd2ce3058b3d2f5e0fd4347ef4e00300e367d1a54c873ad2aa51",
    ),
    (
        "graphics",
        "8bf69d84af79241a94e6770e315ce79dfa9cf1d6f85052af122444c3b94dac5f",
    ),
];

/// The two prefixes a surface's evidence line starts with: a written
/// document's hash, and the read-surface text's.
const EVIDENCE: [&str; 2] = ["WROTE sha256=", "READ sha256="];

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
    /// Commands that must succeed first — the Java surface's compile. One
    /// that fails is a failure of the surface, never a skip: the toolchain
    /// was there, so "could not build" is an answer about the binding.
    prepare: Vec<Command>,
}

impl Surface {
    fn ready(name: &'static str, command: Command) -> Self {
        Self {
            name,
            plan: Ok(command),
            prepare: Vec::new(),
        }
    }

    fn skipped(name: &'static str, why: String) -> Self {
        Self {
            name,
            plan: Err(why),
            prepare: Vec::new(),
        }
    }
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
        go_surface(root, &fixture),
        ruby_surface(root, &fixture),
        java_surface(root, &fixture),
    ];

    let mut ran = Vec::new();
    let mut skipped = Vec::new();
    let mut problems = Vec::new();

    for surface in surfaces {
        match evaluate(surface) {
            (name, Outcome::Ran) => {
                println!("bindings-parity: {name} RAN, every hash agrees");
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
        println!(
            "bindings-parity: every surface agrees ({} of {})",
            ran.len(),
            ran.len()
        );
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

    for mut step in surface.prepare {
        let failed = match step.output() {
            Ok(output) if output.status.success() => None,
            Ok(output) => Some(format!(
                "{:?} exited {}; stderr: {}",
                step.get_program(),
                output.status,
                String::from_utf8_lossy(&output.stderr)
                    .trim()
                    .lines()
                    .next()
                    .unwrap_or("(empty)")
            )),
            Err(error) => Some(format!(
                "{:?} could not be run: {error}",
                step.get_program()
            )),
        };
        if let Some(problem) = failed {
            return (surface.name, Outcome::Failed(vec![problem]));
        }
    }

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
                "printed no `WROTE sha256=` or `READ sha256=` line for {script}, \
                 which is not the same as printing a wrong one — a surface that \
                 silently writes nothing passes every check that only compares \
                 what it printed"
            )),
            Some(actual) if actual != expected => problems.push(format!(
                "{script}: printed {actual}, and the recorded answer is {expected}"
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

/// Every `WROTE sha256=<hex> ... script=<name>` and `READ sha256=<hex> ...
/// script=<name>` line, as script to hash.
///
/// Parsed by key rather than by position so a surface may print whatever else
/// it likes around them — the .NET smoke prefixes its own name, the JavaScript
/// one reports its transaction leg — without this having to know about it.
fn harvest(stdout: &str) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    for line in stdout.lines() {
        let Some(start) = EVIDENCE.iter().filter_map(|prefix| line.find(prefix)).min() else {
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
    Surface::ready("facade", command)
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
        return Surface::skipped(
            "python",
            format!(
                "`{python} -c \"import tinker_pdf\"` failed, so no wheel is \
                 installed for this interpreter; `maturin build` and `pip \
                 install` it, or pass --python"
            ),
        );
    }

    let mut command = Command::new(python);
    command
        .current_dir(root)
        .arg(root.join("bindings/python/tests/write_parity.py"))
        .arg(fixture);
    Surface::ready("python", command)
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
        return Surface::skipped(
            "js",
            format!(
                "{} holds no installed tinker-pdf-js; `wasm-pack build --target \
                 web`, `npm pack` and `npm install` the tarball there, or pass \
                 --node-dir",
                dir.display()
            ),
        );
    }

    let mut command = Command::new("node");
    command
        .current_dir(&dir)
        .arg(root.join("bindings/js/tests/write_parity.mjs"))
        .arg(fixture);
    Surface::ready("js", command)
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
        return Surface::skipped(
            "dotnet",
            format!(
                "{} holds no packed TinkerPdf; `cargo xtask nuget-stage` then \
                 `dotnet pack bindings/dotnet/TinkerPdf.csproj -c Release -o \
                 target/nuget`",
                package.display()
            ),
        );
    }
    let Some(face) = system_face() else {
        return Surface::skipped(
            "dotnet",
            "no system face was found, and the smoke's read leg asserts \
             blank-then-inked before its write leg runs"
                .to_string(),
        );
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
    Surface::ready("dotnet", command)
}

/// The engine as a C library, where `cargo build -p tinker-pdf-ffi --release`
/// leaves it. The three bindings over the C ABI load this file and nothing
/// else, so without it they are skipped, by name, rather than run against
/// whatever a system search path happens to hold.
fn release_library(root: &Path) -> Result<PathBuf, String> {
    let library = root.join("target/release").join(format!(
        "{}tinker_pdf_ffi{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    if library.is_file() {
        Ok(library)
    } else {
        Err(format!(
            "{} is not built; `cargo build -p tinker-pdf-ffi --release`",
            library.display()
        ))
    }
}

/// Whether a program is on this machine, asked the cheapest way it answers.
fn present(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// The Go binding: cgo over the committed header, linked against the release
/// library with that directory as its run-time path.
fn go_surface(root: &Path, fixture: &Path) -> Surface {
    let library = match release_library(root) {
        Ok(library) => library,
        Err(why) => return Surface::skipped("go", why),
    };
    if !present("go", &["version"]) {
        return Surface::skipped("go", "no `go` toolchain on PATH".to_string());
    }
    // The search path is the release directory and nothing else. Under
    // `cargo run` it otherwise holds target/debug/deps, where a test build may
    // have left an older libtinker_pdf_ffi — and the loader prefers the
    // search path to the program's own run path, so the parity program would
    // run against that one and die on the first symbol it lacks.
    let directory = library.parent().unwrap_or(root).to_path_buf();
    let mut command = Command::new("go");
    command
        .current_dir(root.join("bindings/go"))
        .env("LD_LIBRARY_PATH", &directory)
        .env("DYLD_LIBRARY_PATH", &directory)
        .args(["run", "./cmd/parity"])
        .arg(fixture);
    Surface::ready("go", command)
}

/// The Ruby binding: Fiddle, from Ruby's standard library, so there is no gem
/// to install and the interpreter is the whole toolchain.
fn ruby_surface(root: &Path, fixture: &Path) -> Surface {
    let library = match release_library(root) {
        Ok(library) => library,
        Err(why) => return Surface::skipped("ruby", why),
    };
    if !present("ruby", &["-e", "require 'fiddle'"]) {
        return Surface::skipped("ruby", "no `ruby` with Fiddle on PATH".to_string());
    }
    let mut command = Command::new("ruby");
    command
        .current_dir(root.join("bindings/ruby"))
        .env("TINKER_PDF_LIB", &library)
        .args(["-Ilib", "test/write_parity.rb"])
        .arg(fixture);
    Surface::ready("ruby", command)
}

/// The flags `java.lang.foreign` needs on a JDK of this feature release: on
/// 21 it is a preview API, so both the compile and the run name the release
/// and enable previews; from 22 it is final and needs neither. Below 21 it
/// does not exist, which is `None`.
fn jdk_flags(feature: u32) -> Option<(Vec<String>, Vec<String>)> {
    match feature {
        21 => Some((
            vec![
                "--enable-preview".to_string(),
                "--release".to_string(),
                "21".to_string(),
            ],
            vec!["--enable-preview".to_string()],
        )),
        22.. => Some((
            vec!["--release".to_string(), feature.to_string()],
            Vec::new(),
        )),
        _ => None,
    }
}

/// The feature release from `javac -version`'s `javac 21.0.10`.
fn jdk_feature(version: &str) -> Option<u32> {
    let number = version.split_whitespace().nth(1)?;
    number.split(['.', '-', '+']).next()?.parse().ok()
}

/// Every `.java` file under a directory, sorted, so the compile is the same
/// command on every machine.
fn java_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            java_sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "java") {
            found.push(path);
        }
    }
}

/// The Java binding: the Foreign Function and Memory API, compiled here into
/// `target/java-parity` and run with the release library named explicitly.
fn java_surface(root: &Path, fixture: &Path) -> Surface {
    let library = match release_library(root) {
        Ok(library) => library,
        Err(why) => return Surface::skipped("java", why),
    };
    let version = Command::new("javac")
        .arg("-version")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| {
            // javac printed its version on stderr before JDK 9 and stdout
            // since; either way the line is `javac <version>`.
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            text
        });
    let Some(version) = version else {
        return Surface::skipped("java", "no `javac` on PATH".to_string());
    };
    let line = version
        .lines()
        .find(|line| line.starts_with("javac "))
        .unwrap_or("");
    let Some((compile_flags, run_flags)) = jdk_feature(line).and_then(jdk_flags) else {
        return Surface::skipped(
            "java",
            format!(
                "`{}` is older than JDK 21, which is the first with java.lang.foreign",
                line.trim()
            ),
        );
    };

    let classes = root.join("target/java-parity");
    let mut sources = Vec::new();
    java_sources(&root.join("bindings/java/src"), &mut sources);
    java_sources(&root.join("bindings/java/test"), &mut sources);
    sources.sort();
    let mut compile = Command::new("javac");
    compile
        .current_dir(root)
        .args(&compile_flags)
        .arg("-d")
        .arg(&classes)
        .args(&sources);

    let mut command = Command::new("java");
    command
        .current_dir(root)
        .args(&run_flags)
        .arg("--enable-native-access=ALL-UNNAMED")
        .arg(format!("-Dtinkerpdf.library={}", library.display()))
        .arg("-cp")
        .arg(&classes)
        .arg("WriteParity")
        .arg(fixture);
    Surface {
        name: "java",
        plan: Ok(command),
        prepare: vec![compile],
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

    /// The read script's line is evidence on the same terms as a write's: it
    /// is keyed by its script, and a surface that printed the text's hash
    /// under the other prefix would still be counted for the right script.
    #[test]
    fn a_read_line_is_harvested_beside_the_written_ones() {
        let stdout = "\
WROTE sha256=aaaa surface=go script=fill-and-save bytes=10
GO-PARITY: READ sha256=cccc surface=go script=read-surface bytes=30
";
        let found = harvest(stdout);
        assert_eq!(found.get("read-surface").map(String::as_str), Some("cccc"));
        assert_eq!(found.len(), 2);
        assert!(harvest("READ sha256=cccc surface=go\n").is_empty());
    }

    /// A line without a `script=` is not a hash about anything, so it is not
    /// counted — otherwise a malformed line would satisfy the presence check
    /// for whichever script happened to be missing.
    #[test]
    fn a_line_with_no_script_is_not_evidence() {
        assert!(harvest("WROTE sha256=aaaa surface=facade\n").is_empty());
        assert!(harvest("nothing here at all\n").is_empty());
    }

    /// JDK 21 needs the preview flags on both sides, 22 and later need
    /// neither, and anything older has no java.lang.foreign to bind with.
    #[test]
    fn the_jdk_feature_release_picks_the_flags() {
        assert_eq!(jdk_feature("javac 21.0.10"), Some(21));
        assert_eq!(jdk_feature("javac 22"), Some(22));
        assert_eq!(jdk_feature("javac 23-ea"), Some(23));
        assert_eq!(jdk_feature("javac 1.8.0_402"), Some(1));
        assert_eq!(jdk_feature("nonsense"), None);

        let (compile, run) = jdk_flags(21).expect("21 has the preview API");
        assert_eq!(compile, ["--enable-preview", "--release", "21"]);
        assert_eq!(run, ["--enable-preview"]);
        let (compile, run) = jdk_flags(22).expect("22 has the final API");
        assert_eq!(compile, ["--release", "22"]);
        assert!(run.is_empty());
        assert!(jdk_flags(17).is_none());
        assert!(jdk_flags(1).is_none());
    }

    /// The two scripts are named once and the names are what every surface
    /// prints, so a rename that reached only one of them is a mismatch rather
    /// than a silent pass.
    #[test]
    fn the_recorded_answer_names_both_scripts_once() {
        assert_eq!(EXPECTED.len(), 12);
        let names: Vec<&str> = EXPECTED.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            [
                "fill-and-save",
                "build-a-document",
                "document-ops",
                "sanitise",
                "save-options",
                "save-linearized",
                "sanitise-report",
                "read-surface",
                "signatures",
                "forms",
                "form-data",
                "graphics"
            ]
        );
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
