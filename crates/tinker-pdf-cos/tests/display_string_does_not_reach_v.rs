//! **The proof that a format action's display string cannot reach `/V`.**
//!
//! 12.7.3.3 keeps a field's value and its appearance apart. Until
//! `DisplayString` existed, this engine kept them apart because
//! `calc::formatted_value` happened not to call the editor — a property of
//! how the code was arranged, not a rule anything enforced. The first caller
//! to write `editor.set_field_value(name, formatted)` would have produced a
//! form whose `/V` reads "GBP 1,234.00" where a consumer expects 1234, and
//! nothing in the workspace would have objected.
//!
//! A comment claiming a type enforces something is not that enforcement, and
//! neither is a test that exercises the calls that happen to exist. So this
//! compiles the mistake. Five builds: **the pristine caller first**, which is
//! what makes the rest assertions rather than a harness that would report
//! success for a snippet that could never build; then the same caller with
//! the display string handed to each of the four doors into the document, and
//! each one asserted to fail with `error[E0308]`.
//!
//! # Why `rustc`, and why a caller rather than the crate
//!
//! `crates/tinker-pdf-css/tests/unimplemented_property_does_not_build.rs`
//! compiles the whole css crate, because the defect it injects lives *inside*
//! that crate — a non-exhaustive `match`. This defect lives at the **call
//! site**, so the thing that must fail to compile is a caller, and the
//! smallest honest caller is the one below linked against the real
//! `tinker-pdf-cos` rlib this test run just built. Nothing here is a copy of
//! the type; the `DisplayString` that refuses is the one that ships.
//!
//! `tinker-pdf-cos` has five internal dependencies, so a bare `rustc` cannot
//! rebuild it from source the way the css proof does. It does not need to:
//! `--extern` against the built rlib and `-L dependency` over cargo's own
//! `deps` directory is the same compiler asking the same question. Running
//! `cargo` inside `cargo test` would contend for the `target/` lock, which on
//! Windows surfaces as `LNK1104` and reads exactly like a compile error.
//!
//! # Which rlib
//!
//! `deps/` holds several `libtinker_pdf_cos-*.rlib`, one per feature set and
//! profile cargo has built, and picking the newest by timestamp is a guess.
//! So the pristine control **is** the selection: each candidate is tried in
//! turn and the first one the control compiles against is the one the
//! injections use. If none of them compiles the control, this file fails and
//! says so with every compiler error it collected — a proof that quietly does
//! not run reads exactly like a proof that passed.
//!
//! # What the type does not claim
//!
//! `DisplayString::text` exists, and `editor.set_field_value(name,
//! formatted.text())` compiles. That is deliberate: a caller has to be able
//! to draw the characters, and no type can stop somebody who means it. What
//! the type removes is the mistake nobody makes on purpose — and `.text()` is
//! a spelling a reviewer can see.

use std::path::PathBuf;
use std::process::Command;

/// Where the injection goes, and it is not in this string's own source twice.
const ANCHOR: &str = "// <<< the display-string proof puts one line here >>>";

/// The caller. Everything above the anchor is what any host would write.
const CALLER: &str = r#"
use tinker_pdf_cos::{calc, DocumentEditor};

pub fn show(editor: &mut DocumentEditor, name: &str) {
    let Ok(Some(formatted)) = calc::formatted_value(editor, name) else {
        return;
    };
    // <<< the display-string proof puts one line here >>>
}
"#;

/// The control: the one spelling that is meant to work.
const PRISTINE: &str = "    let _ = editor.set_field_value(name, formatted.text());";

/// The four doors into the document, each injected on its own. One `match`
/// is one consequence, and one write door is one guarantee: a build where
/// three refuse and the fourth accepts is a build where the rule holds
/// everywhere except the place somebody happened to call.
const DOORS: [(&str, &str); 4] = [
    (
        "set_field_value",
        "    let _ = editor.set_field_value(name, formatted);",
    ),
    (
        "fill_field",
        "    let _ = editor.fill_field(name, formatted);",
    ),
    (
        "set_field_values",
        "    let _ = editor.set_field_values(&[(name, formatted)]);",
    ),
    (
        "set_calculated_values",
        "    let _ = editor.set_calculated_values(&[(name, formatted)]);",
    ),
];

fn deps_dir() -> PathBuf {
    // The test binary lives in `<target>/<profile>/deps`, which is also where
    // every rlib this run built landed. Asking the running executable is the
    // only answer that cannot be wrong about the profile.
    let exe = std::env::current_exe().expect("the test binary knows where it is");
    exe.parent()
        .expect("a test binary has a directory")
        .to_path_buf()
}

/// Every `libtinker_pdf_cos-*.rlib` cargo has produced, newest first.
fn candidates() -> Vec<PathBuf> {
    let deps = deps_dir();
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let entries = std::fs::read_dir(&deps)
        .unwrap_or_else(|e| panic!("{}: {e}", deps.display()))
        .flatten();
    for entry in entries {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("libtinker_pdf_cos-") || !name.ends_with(".rlib") {
            continue;
        }
        let when = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        found.push((when, path));
    }
    assert!(
        !found.is_empty(),
        "no libtinker_pdf_cos rlib in {}, so nothing here could be compiled against \
         the real type",
        deps.display()
    );
    found.sort_by_key(|(when, _)| std::cmp::Reverse(*when));
    found.into_iter().map(|(_, path)| path).collect()
}

fn inject(line: &str) -> String {
    assert_eq!(
        CALLER.matches(ANCHOR).count(),
        1,
        "the anchor is not in the caller exactly once"
    );
    CALLER.replace(ANCHOR, line.trim_start_matches("    "))
}

/// Compiles one caller against one rlib, and answers what the compiler said.
fn compile(name: &str, source: &str, rlib: &std::path::Path) -> (bool, String) {
    let scratch = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("the scratch directory");
    let file = scratch.join("caller.rs");
    std::fs::write(&file, source).expect("write");

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let output = Command::new(rustc)
        .args(["--edition", "2021"])
        .args(["--crate-type", "lib"])
        .args(["--crate-name", "display_proof"])
        .arg("--emit=metadata")
        .arg("--extern")
        .arg(format!("tinker_pdf_cos={}", rlib.display()))
        .arg("-L")
        .arg(format!("dependency={}", deps_dir().display()))
        .arg("--out-dir")
        .arg(&scratch)
        .arg(&file)
        .output()
        .expect(
            "rustc could not be run. It is not optional and this test does not skip: a proof \
             that quietly does not run reads exactly like a proof that passed",
        );
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The rlib the control compiles against, chosen by compiling the control.
///
/// Behind a `OnceLock` because the tests below run in parallel and this
/// writes into a scratch directory it first removes — two of them racing
/// there fails with a missing path rather than with anything about the type,
/// which is exactly the harness noise this file exists to keep out.
fn working_rlib() -> &'static PathBuf {
    static CHOSEN: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    CHOSEN.get_or_init(|| {
        let mut refused = String::new();
        for rlib in candidates() {
            let (ok, stderr) = compile("display-proof-pristine", &inject(PRISTINE), &rlib);
            if ok {
                return rlib;
            }
            refused.push_str(&format!("\n--- {} ---\n{stderr}", rlib.display()));
        }
        panic!(
            "the pristine caller compiled against none of the built rlibs, so nothing below \
             proves anything:{refused}"
        );
    })
}

/// **The proof.** The one legitimate spelling builds; handing the display
/// string to any of the four write doors does not.
#[test]
fn a_display_string_builds_through_text_and_reaches_no_write_door() {
    let rlib = working_rlib();

    for (door, line) in DOORS {
        let (ok, stderr) = compile(&format!("display-proof-{door}"), &inject(line), rlib);
        assert!(
            !ok,
            "`{door}` accepted a format action's display string, which is 12.7.3.3 \
             not holding"
        );
        assert!(
            stderr.contains("E0308"),
            "`{door}` failed for some other reason than a type mismatch:\n{stderr}"
        );
        assert!(
            stderr.contains("DisplayString"),
            "the error does not name the type that refused:\n{stderr}"
        );
    }
}

/// And the other direction, said separately: the control is not merely "some
/// caller compiles", it is *this* caller with `.text()` in it. Without this a
/// change that broke every one of the five builds would pass the test above.
#[test]
fn the_control_that_makes_the_refusals_mean_something_compiles() {
    let rlib = working_rlib();
    let (ok, stderr) = compile("display-proof-control", &inject(PRISTINE), rlib);
    assert!(ok, "the pristine caller stopped compiling:\n{stderr}");
}
