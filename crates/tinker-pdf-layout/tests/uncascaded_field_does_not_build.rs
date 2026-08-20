//! A computed property with no layout consumer **does not compile**.
//!
//! Gap 31's decision 5 is the device its whole reflowable scope was accepted
//! on: *a property is a parser variant only when a consumer exists*, enforced
//! by three exhaustive `match`es in `tinker-pdf-css` so that a property parsed
//! and never cascaded is `error[E0004]`.
//!
//! That guard stops one milestone short of the page. Milestone 6's own
//! `Still owed` says so — *"nothing consumes any of this, which is milestone
//! 7's and milestone 8's"* — and the gap it leaves is the same failure one
//! level up: a property can be parsed, cascaded, written into `ComputedStyle`,
//! and then read by nobody. The book still renders. It renders slightly
//! differently, and nobody can tell by looking.
//!
//! `style::consume` closes it by destructuring `ComputedStyle` with **no
//! `..`**, and this file is the proof that the destructure is load-bearing
//! rather than a style choice.
//!
//! # How it can be a real build rather than a sketch
//!
//! `tinker-pdf-css` has an empty DAG allow-list, no third-party dependency and
//! no build script, so the whole crate compiles with a bare `rustc` — which is
//! what made milestone 6's proof possible. This one adds one step: compile the
//! css crate to metadata, then compile the **real** `style.rs`, `metrics.rs`,
//! `uax14.rs` and `unicode.rs` against it with `--extern`. Nothing here is a
//! copy of the pattern; the file that fails to build is the file that ships.
//!
//! # Four builds, and three of them are controls
//!
//! - **The pristine pair compiles.** Without this, a harness that wrote a
//!   broken tree — a bad path, a `rustc` that is not on `PATH`, an `OUT_DIR`
//!   that does not hold the generated tables — would report every injection as
//!   *"the build failed"*, which is the answer they are asserting, and the
//!   whole file would pass while proving nothing. Gap 20's finding, arriving
//!   through a harness instead of through CI.
//! - **The same field with a binding supplied compiles.** Otherwise the
//!   failure below is true of any edit at all, and *"the build broke"* is not
//!   *"the build broke because the property has no consumer"*.
//! - **The field with no binding fails, and fails with `E0027`.** A build that
//!   failed for some other reason would pass a test that only asked whether it
//!   failed.
//! - **And the error names the field**, so a future reader knows which one.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the anchors live.
const FIELD_ANCHOR: &str = "// <<< the layout proof injects a field directly above this line >>>";
const INITIAL_ANCHOR: &str = "// <<< the layout proof's initial value goes here >>>";
const BINDING_ANCHOR: &str = "// <<< the layout proof's binding goes here >>>";

/// The property injected, and it is not an arbitrary one.
///
/// `hyphens` is in `UNSUPPORTED_PROPERTIES` today — `css-text-4`, written by
/// both of milestone 1's producers, and genuinely unimplemented because
/// hyphenation needs a dictionary this engine does not have. So the defect is
/// the exact edit somebody will make when that arrives: promote the name out of
/// the unsupported list, add the variant, add the three `match` arms the css
/// crate demands, add the field — and stop, one door short of the page.
const FIELD: &str = "    pub hyphens: bool,";
const INITIAL: &str = "            hyphens: false,";
const BINDING: &str = "        hyphens,";

fn css_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .join("tinker-pdf-css")
}

fn layout_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn rustc() -> Command {
    Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string()))
}

/// Replaces an anchor, and panics if it was not there exactly once.
///
/// A silent no-op would make every injection a copy of the pristine build, and
/// every one of them would report success. Gap 31 milestone 5 lost a whole pass
/// to guessed anchors; this one refuses to guess.
fn inject(source: &str, anchor: &str, line: &str) -> String {
    assert_eq!(
        source.matches(anchor).count(),
        1,
        "the anchor `{anchor}` is not in the source exactly once"
    );
    source.replace(anchor, &format!("{}\n{anchor}", line.trim_end()))
}

/// Compiles `tinker-pdf-css` — with or without the extra field — to metadata,
/// and answers where the `.rmeta` landed.
fn build_css(name: &str, with_field: bool) -> PathBuf {
    let root = css_root();
    let scratch = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let src = scratch.join("src");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&src).expect("the scratch directory");
    for file in [
        "lib.rs",
        "limits.rs",
        "media.rs",
        "parser.rs",
        "property.rs",
        "selector.rs",
        "tokenizer.rs",
    ] {
        std::fs::write(src.join(file), read(&root.join("src").join(file))).expect("write");
    }
    let mut cascade = read(&root.join("src/cascade.rs"));
    if with_field {
        cascade = inject(&cascade, FIELD_ANCHOR, FIELD);
        cascade = inject(&cascade, INITIAL_ANCHOR, INITIAL);
    }
    std::fs::write(src.join("cascade.rs"), cascade).expect("write");

    let output = rustc()
        .args(["--edition", "2021"])
        .args(["--crate-type", "lib"])
        .args(["--crate-name", "tinker_pdf_css"])
        .arg("--emit=metadata")
        .arg("--out-dir")
        .arg(&scratch)
        .arg(src.join("lib.rs"))
        .output()
        .expect(
            "rustc could not be run. It is not optional and this test does not skip: a proof \
             that quietly does not run reads exactly like a proof that passed",
        );
    assert!(
        output.status.success(),
        "the css crate did not build, so nothing below proves anything:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    scratch.join("libtinker_pdf_css.rmeta")
}

/// Compiles the real layout modules against a given css, and answers whether
/// it built and what it said.
fn build_layout(name: &str, css: &Path, with_binding: bool) -> (bool, String) {
    let root = layout_root();
    let scratch = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let src = scratch.join("src");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&src).expect("the scratch directory");

    // The four real files, verbatim but for the one anchor. `unicode.rs`
    // carries the `include!` of the generated tables and `OUT_DIR` is passed
    // through below, so the build below reads this crate's own build script's
    // output rather than a stand-in for it.
    for file in ["metrics.rs", "uax14.rs", "unicode.rs"] {
        std::fs::write(src.join(file), read(&root.join("src").join(file))).expect("write");
    }
    let mut style = read(&root.join("src/style.rs"));
    if with_binding {
        style = inject(&style, BINDING_ANCHOR, BINDING);
    }
    std::fs::write(src.join("style.rs"), style).expect("write");
    // A root that declares exactly the modules `style.rs` reaches for. It is
    // four lines because everything with any behaviour in it is the real file
    // beside it.
    std::fs::write(
        src.join("lib.rs"),
        "pub mod metrics;\npub mod style;\npub mod uax14;\npub mod unicode;\n",
    )
    .expect("write");

    let output = rustc()
        .args(["--edition", "2021"])
        .args(["--crate-type", "lib"])
        .args(["--crate-name", "tinker_pdf_layout"])
        .arg("--emit=metadata")
        .arg("--extern")
        .arg(format!("tinker_pdf_css={}", css.display()))
        .arg("--out-dir")
        .arg(&scratch)
        .arg(src.join("lib.rs"))
        // `unicode.rs` is `include!(concat!(env!("OUT_DIR"), "/ucd.rs"))`, and
        // an integration test of a crate with a build script is compiled with
        // `OUT_DIR` set — so the child compiler is handed the same tables the
        // real build uses.
        .env("OUT_DIR", env!("OUT_DIR"))
        .output()
        .expect("rustc could not be run");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The control: the tree as it stands builds.
#[test]
fn the_unmodified_pair_builds() {
    let css = build_css("layout-proof-pristine-css", false);
    let (ok, stderr) = build_layout("layout-proof-pristine", &css, false);
    assert!(
        ok,
        "the unmodified crates did not build, so nothing else here proves anything:\n{stderr}"
    );
}

/// The second control: the same field, with a binding, builds.
///
/// Without this the failure below would be true of any edit, and the file
/// would be asserting that `rustc` can fail rather than that decision 5 holds.
#[test]
fn the_same_field_with_a_binding_builds() {
    let css = build_css("layout-proof-supplied-css", true);
    let (ok, stderr) = build_layout("layout-proof-supplied", &css, true);
    assert!(
        ok,
        "a field with a binding did not build, so the failure below is about something \
         else:\n{stderr}"
    );
}

/// The proof: a cascaded property that layout does not read fails the
/// **build**, by name.
#[test]
fn a_computed_field_with_no_layout_consumer_does_not_build() {
    let css = build_css("layout-proof-defect-css", true);
    let (ok, stderr) = build_layout("layout-proof-defect", &css, false);
    assert!(
        !ok,
        "a property that is cascaded and never laid out compiled, which is decision 5 not \
         holding one milestone past the crate that invented it"
    );
    assert!(
        stderr.contains("E0027"),
        "the build failed for some other reason than a pattern that does not mention a \
         field:\n{stderr}"
    );
    assert!(
        stderr.contains("hyphens"),
        "the error does not name the field that was added:\n{stderr}"
    );
}
