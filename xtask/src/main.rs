//! Repository chores that need more than a cargo command.
//!
//! `cargo xtask <task>`. Every task exits non-zero on failure so CI can run it
//! without a wrapper.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
xtask — repository chores

usage:
  cargo xtask dag     check the crate dependency graph against the declared one
  cargo xtask libm    check that no pixel path calls the platform's libm
  cargo xtask check   both of the above
  cargo xtask help
";

fn main() -> ExitCode {
    let task = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "help".to_string());
    match task.as_str() {
        "dag" => report("dag", check_dag()),
        "libm" => report("libm", check_libm()),
        "check" => {
            let dag = check_dag();
            let libm = check_libm();
            let mut problems = dag.err().unwrap_or_default();
            problems.extend(libm.err().unwrap_or_default());
            report(
                "check",
                if problems.is_empty() {
                    Ok(())
                } else {
                    Err(problems)
                },
            )
        }
        "help" | "-h" | "--help" => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("xtask: unknown task `{other}`");
            print!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// The dependency graph the architecture plan declares, as
/// `crate -> everything it may depend on`.
///
/// Ruling 8 and plan 00: the leaves take bytes and return values and know no
/// PDF types, which is what makes each of them independently fuzzable and
/// keeps the layering honest. Nothing here is checked by the compiler — a new
/// edge compiles perfectly well — so it is checked here instead.
///
/// **`cos -> font` is a deliberate amendment to the plan's DAG.** The plan
/// listed `cos -> {filters, crypto}`. Reading a font *dictionary* — its
/// encoding, its `/ToUnicode`, its standard-14 metrics — is COS work, and it
/// needs the leaf font crate's CMap and encoding tables to do it. The
/// alternative is a third crate between them whose only job is to hold two
/// tables, which is worse. The edge points from a higher layer to a leaf, so
/// it does not invert the layering.
///
/// **`math` is a second-order leaf.** Deterministic `sin`, `ln` and `pow`
/// belong *under* the rasteriser and the colour code rather than inside
/// either, because both need them and neither may depend on the other. It has
/// no dependencies of its own — not even `std` — so it adds a layer below the
/// leaves rather than an edge between them.
const ALLOWED: &[(&str, &[&str])] = &[
    // The bottom: nothing at all, internal or otherwise.
    ("tinker-pdf-math", &[]),
    // Leaves: nothing internal beyond the maths.
    ("tinker-pdf-filters", &[]),
    ("tinker-pdf-crypto", &[]),
    ("tinker-pdf-font", &[]),
    ("tinker-pdf-color", &["tinker-pdf-math"]),
    ("tinker-pdf-raster", &["tinker-pdf-math"]),
    // File syntax and the object model.
    (
        "tinker-pdf-cos",
        &["tinker-pdf-filters", "tinker-pdf-crypto", "tinker-pdf-font"],
    ),
    // Content interpretation emits to a `Device`; it never rasterizes.
    (
        "tinker-pdf-content",
        &["tinker-pdf-cos", "tinker-pdf-font", "tinker-pdf-color"],
    ),
    (
        "tinker-pdf-render",
        &[
            "tinker-pdf-content",
            "tinker-pdf-raster",
            "tinker-pdf-font",
            "tinker-pdf-color",
        ],
    ),
    // The facade may reach anything; it is what users depend on.
    (
        "tinker-pdf",
        &[
            "tinker-pdf-cos",
            "tinker-pdf-font",
            "tinker-pdf-crypto",
            "tinker-pdf-content",
            "tinker-pdf-render",
            "tinker-pdf-raster",
            "tinker-pdf-filters",
            "tinker-pdf-color",
        ],
    ),
    // Ruling 11: bindings sit on the facade only.
    ("tinker-pdf-ffi", &["tinker-pdf"]),
];

/// Crates outside `crates/` that may depend on the facade, by path from the
/// repository root.
///
/// Paths rather than bare names because `xtask` does not live under `tools/`.
/// It was listed as `"xtask"` and looked for at `tools/xtask/Cargo.toml`,
/// which does not exist, so the manifest read failed, the loop moved on, and
/// xtask's own dependencies were never checked at all — by a check whose
/// entire purpose is that the compiler cannot do this.
const TOOLS: &[&str] = &["tools/pdfcmp", "tools/oracle-diff", "tools/tpdf", "xtask"];

fn repo_root() -> PathBuf {
    // The manifest directory is `<root>/xtask`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Prints a task's outcome and turns it into an exit code.
fn report(task: &str, outcome: Result<(), Vec<String>>) -> ExitCode {
    match outcome {
        Ok(()) => {
            println!("{task}: ok");
            ExitCode::SUCCESS
        }
        Err(problems) => {
            for problem in &problems {
                eprintln!("{task}: {problem}");
            }
            eprintln!("{task}: {} problem(s)", problems.len());
            ExitCode::FAILURE
        }
    }
}

/// The crates whose output is pixels, and which therefore may not call the
/// platform's transcendental functions.
///
/// Ruling 4 wants byte-identical rendering across targets. `sqrt` and the
/// rounding family are safe — IEEE 754 requires them to be correctly rounded,
/// so every platform agrees. The functions below are not, and glibc, musl,
/// the MSVC runtime, Apple's libm and the wasm shim each round them their own
/// way. `tinker-pdf-math` exists to replace them; this makes sure nobody
/// quietly goes back.
const PIXEL_PATHS: &[&str] = &[
    "tinker-pdf-raster",
    "tinker-pdf-color",
    "tinker-pdf-render",
    "tinker-pdf-content",
];

/// Method calls that are not correctly rounded, and so differ between
/// platforms. Spelled with the dot so `f.exp()` matches and a local named
/// `exp` does not.
const FORBIDDEN: &[&str] = &[
    ".sin()",
    ".cos()",
    ".tan()",
    ".asin()",
    ".acos()",
    ".atan()",
    ".atan2(",
    ".sinh()",
    ".cosh()",
    ".tanh()",
    ".exp()",
    ".exp2()",
    ".exp_m1()",
    ".ln()",
    ".ln_1p()",
    ".log(",
    ".log2()",
    ".log10()",
    ".powf(",
    ".cbrt()",
    ".hypot(",
    ".to_radians()",
    ".to_degrees()",
    ".mul_add(",
];

fn check_libm() -> Result<(), Vec<String>> {
    let root = repo_root();
    let mut problems = Vec::new();

    for crate_name in PIXEL_PATHS {
        let src = root.join("crates").join(crate_name).join("src");
        let mut files = Vec::new();
        collect_rust_files(&src, &mut files);

        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            // Tests may compare against the platform — that is how the maths
            // crate proves it agrees with one. Only shipped code is bound.
            let shipped = text.split("#[cfg(test)]").next().unwrap_or(&text);

            for (number, line) in shipped.lines().enumerate() {
                let code = line.split("//").next().unwrap_or(line);
                for call in FORBIDDEN {
                    if code.contains(call) {
                        let shown = file
                            .strip_prefix(&root)
                            .unwrap_or(&file)
                            .display()
                            .to_string();
                        problems.push(format!(
                            "{shown}:{}: `{call}` is not correctly rounded, so it differs between platforms; use tinker_pdf_math (ruling 4)",
                            number + 1
                        ));
                    }
                }
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn check_dag() -> Result<(), Vec<String>> {
    let root = repo_root();
    let crates_dir = root.join("crates");

    let mut problems = Vec::new();
    let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let entries = match std::fs::read_dir(&crates_dir) {
        Ok(entries) => entries,
        Err(e) => return Err(vec![format!("cannot read {}: {e}", crates_dir.display())]),
    };

    for entry in entries.flatten() {
        let manifest = entry.path().join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        let Some(name) = package_name(&text) else {
            problems.push(format!("{} has no [package] name", manifest.display()));
            continue;
        };
        seen.insert(name, internal_dependencies(&text));
    }

    for (name, deps) in &seen {
        let Some((_, allowed)) = ALLOWED.iter().find(|(n, _)| n == name) else {
            problems.push(format!(
                "{name} is not in the declared graph — add it to ALLOWED with the \
                 reason, or it is an accident"
            ));
            continue;
        };
        for dep in deps {
            if !allowed.contains(&dep.as_str()) {
                problems.push(format!(
                    "{name} -> {dep} is not a declared edge (plan 00, ruling 8)"
                ));
            }
        }
    }

    for (name, _) in ALLOWED {
        if !seen.contains_key(*name) {
            problems.push(format!("{name} is declared but no such crate exists"));
        }
    }

    // The tools and bindings are checked only for the one rule that matters
    // for them: ruling 11 keeps a binding on the facade alone.
    for tool in TOOLS {
        let manifest = root.join(tool).join("Cargo.toml");
        let text = match std::fs::read_to_string(&manifest) {
            Ok(text) => text,
            // Not a skip. A check that quietly passes over what it cannot
            // find is a check that does not run, and this one already spent
            // its whole life doing exactly that to `xtask`.
            Err(error) => {
                problems.push(format!(
                    "{tool}/Cargo.toml could not be read ({error}), so its \
                     dependencies went unchecked"
                ));
                continue;
            }
        };
        for dep in internal_dependencies(&text) {
            if dep != "tinker-pdf" {
                problems.push(format!(
                    "{tool} -> {dep}: tools use the facade, so that they \
                     exercise what users get"
                ));
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The `name = "..."` of the `[package]` table.
///
/// Hand-rolled rather than parsed with a TOML crate: this reads two shapes of
/// line out of manifests this repository writes, and adding a dependency to
/// check the dependency rules would be its own kind of funny.
fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package {
            if let Some(value) = line.strip_prefix("name") {
                let value = value.trim_start().strip_prefix('=')?.trim();
                return Some(value.trim_matches('"').to_string());
            }
        }
    }
    None
}

/// Every `tinker-pdf*` dependency named in any dependency table.
fn internal_dependencies(manifest: &str) -> Vec<String> {
    let mut in_deps = false;
    let mut out = Vec::new();

    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            // `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`
            // and their target-specific forms all count: a dev-dependency edge
            // is still an edge for the purpose of "who knows about whom".
            in_deps = line.contains("dependencies]");
            continue;
        }
        if !in_deps || line.starts_with('#') || line.is_empty() {
            continue;
        }
        let Some(name) = line.split('=').next().map(str::trim) else {
            continue;
        };
        if name.starts_with("tinker-pdf") {
            out.push(name.to_string());
        }
    }

    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_package_name_is_read() {
        let manifest = "[package]\nname = \"tinker-pdf-cos\"\nversion = \"0.1.0\"\n";
        assert_eq!(package_name(manifest).as_deref(), Some("tinker-pdf-cos"));
    }

    /// A `name` key in some other table is not the package's.
    #[test]
    fn a_name_outside_the_package_table_is_ignored() {
        let manifest = "[lib]\nname = \"wrong\"\n\n[package]\nname = \"right\"\n";
        assert_eq!(package_name(manifest).as_deref(), Some("right"));
    }

    #[test]
    fn internal_dependencies_are_found_in_every_table() {
        let manifest = "[dependencies]\ntinker-pdf-cos = { workspace = true }\n\
                        serde = \"1\"\n\n[dev-dependencies]\ntinker-pdf-font = \"0.1\"\n";
        assert_eq!(
            internal_dependencies(manifest),
            vec!["tinker-pdf-cos".to_string(), "tinker-pdf-font".to_string()]
        );
    }

    #[test]
    fn external_dependencies_are_not_edges() {
        let manifest = "[dependencies]\nproptest = \"1\"\nlibfuzzer-sys = \"0.4\"\n";
        assert!(internal_dependencies(manifest).is_empty());
    }

    /// The check runs against this repository, which is the point of it.
    #[test]
    fn this_repository_obeys_its_own_graph() {
        if let Err(problems) = check_dag() {
            panic!("the declared graph and the manifests disagree:\n{problems:#?}");
        }
    }
}
