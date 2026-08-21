//! **The proof that a property with no consumer does not build.**
//!
//! Gap 31's decision 5 is the device the whole reflowable scope was accepted
//! on, and its exit criterion is unusual: the defect is *"injected as a defect
//! and asserted to fail the build, not a test"*. A comment claiming the
//! compiler enforces something is not that enforcement, and neither is a test
//! that exercises the properties that happen to exist — every one of those
//! passes on the day somebody adds a variant and forgets a `match` arm, because
//! by then the code does not compile and no test runs at all.
//!
//! So this compiles the crate. Twice, at least: **the pristine source first**,
//! which is what makes the result an assertion rather than a harness that would
//! report success for a copy of the source that could never build; then the
//! same source with one variant added to [`tinker_pdf_css::property::Property`]
//! and one consumer's arm withheld, asserted to fail with `error[E0004]` at
//! that consumer.
//!
//! # Why `rustc` and not `cargo`
//!
//! `tinker-pdf-css`'s allow-list is empty — no internal dependency, no
//! third-party one, no build script — so the whole crate compiles with a bare
//! `rustc` and no dependency resolution at all. That is what lets the proof
//! build the **real** source rather than a small copy of the pattern, and it is
//! a property of the DAG amendment rather than a convenience. Running `cargo`
//! inside `cargo test` would also contend for the same `target/` lock, which on
//! Windows surfaces as `LNK1104` and reads exactly like a compile error.
//!
//! `#[cfg(test)] mod tests;` is not compiled here, because this is a plain
//! `--crate-type lib` build and not a `--test` one. The proof is about the
//! library.
//!
//! # The three consumers, injected separately
//!
//! One `match` is one consequence, and gap 31's own rule is that a test for one
//! of two independent consequences is not a test. [`Property`] has three
//! exhaustive consumers and each is withheld on its own:
//!
//! - `cascade::apply`, which is the one the plan names — a property that is
//!   parsed and never written into a computed style;
//! - `Property::name`, without which a property would be applied and then
//!   anonymous in every warning and every census;
//! - `Property::inherited`, without which it would be applied, named, and
//!   inherit or not by accident.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the anchors live, and what each one is for.
const APPLY_ANCHOR: &str = "// <<< the compile-time proof's fourth arm goes here >>>";
const NAME_ANCHOR: &str = "// <<< the compile-time proof's second arm goes here >>>";
const INHERITED_ANCHOR: &str = "// <<< the compile-time proof's third arm goes here >>>";
const VARIANT_ANCHOR: &str =
    "// <<< the compile-time proof injects a variant directly above this line >>>";

/// The property injected, and it is not an arbitrary one.
///
/// It was `widows` when this file was written, for a stated reason: *"the
/// defect is the exact edit somebody will make when that milestone arrives"*.
/// Milestone 7 arrived, `widows` is implemented, and the injection stopped
/// being a defect and became a duplicate variant — which fails the build for
/// the wrong reason and would have made the control build fail too. **That the
/// choice had to move is the evidence it was the right kind of choice**, and
/// this is the third time in this gap that a test written a milestone early has
/// had to be resolved by the milestone it was written for.
///
/// `border-collapse` is the successor and it is chosen the same way: CSS 2.2
/// §17.6, in `UNSUPPORTED_PROPERTIES` today, present in both producers' books,
/// and genuinely not implemented because the table model that would consume it
/// is milestone 11's. When that milestone lands this constant moves again, and
/// the reason it can be moved without weakening anything is that the *shape* of
/// the proof is in the harness rather than in the name.
const VARIANT: &str = "    BorderCollapse(bool),";
const APPLY_ARM: &str = "        Property::BorderCollapse(_) => {}";
const NAME_ARM: &str = "            Property::BorderCollapse(_) => \"border-collapse\",";
const INHERITED_ARM: &str = "            Property::BorderCollapse(_) => true,";

struct Source {
    lib: String,
    property: String,
    cascade: String,
}

impl Source {
    /// The crate's own source, read from the manifest directory.
    fn pristine() -> Self {
        let root = crate_root();
        Self {
            lib: read(&root.join("src/lib.rs")),
            property: read(&root.join("src/property.rs")),
            cascade: read(&root.join("src/cascade.rs")),
        }
    }

    /// Writes this source into a directory and compiles it, returning the
    /// compiler's stderr and whether it succeeded.
    fn compile(&self, name: &str) -> (bool, String) {
        let root = crate_root();
        let scratch = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
        let src = scratch.join("src");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(src.join("tests")).expect("the scratch directory");

        // Everything but the three files the injection touches is copied
        // verbatim, so the proof builds this crate and not a sketch of it.
        for file in [
            "font_face.rs",
            "limits.rs",
            "media.rs",
            "parser.rs",
            "selector.rs",
            "tokenizer.rs",
        ] {
            std::fs::write(src.join(file), read(&root.join("src").join(file))).expect("write");
        }
        std::fs::write(src.join("lib.rs"), &self.lib).expect("write");
        std::fs::write(src.join("property.rs"), &self.property).expect("write");
        std::fs::write(src.join("cascade.rs"), &self.cascade).expect("write");

        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
        let output = Command::new(rustc)
            .arg("--edition")
            .arg("2021")
            .arg("--crate-type")
            .arg("lib")
            .arg("--crate-name")
            .arg("tinker_pdf_css")
            .arg("--emit=metadata")
            .arg("--out-dir")
            .arg(&scratch)
            .arg(src.join("lib.rs"))
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
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Replaces an anchor, and panics if it was not there.
///
/// A silent no-op here would make every injection below a copy of the pristine
/// build, and every one of them would then report the build succeeding — which
/// is the failure this whole file is written to prevent, arriving through the
/// harness instead of through the code. Gap 31 milestone 5 lost a pass to
/// guessed anchors; this one refuses to guess.
fn inject(source: &str, anchor: &str, replacement: &str) -> String {
    assert_eq!(
        source.matches(anchor).count(),
        1,
        "the anchor `{anchor}` is not in the source exactly once"
    );
    source.replace(anchor, replacement)
}

/// A variant added, with the arms named in `arms` supplied and the rest not.
fn with_variant(supply_apply: bool, supply_name: bool, supply_inherited: bool) -> Source {
    let pristine = Source::pristine();
    let mut property = inject(
        &pristine.property,
        VARIANT_ANCHOR,
        &format!("{VARIANT}\n    {VARIANT_ANCHOR}"),
    );
    property = inject(
        &property,
        NAME_ANCHOR,
        &if supply_name {
            format!("{NAME_ARM}\n            {NAME_ANCHOR}")
        } else {
            NAME_ANCHOR.to_string()
        },
    );
    property = inject(
        &property,
        INHERITED_ANCHOR,
        &if supply_inherited {
            format!("{INHERITED_ARM}\n            {INHERITED_ANCHOR}")
        } else {
            INHERITED_ANCHOR.to_string()
        },
    );
    let cascade = inject(
        &pristine.cascade,
        APPLY_ANCHOR,
        &if supply_apply {
            format!("{APPLY_ARM}\n        {APPLY_ANCHOR}")
        } else {
            APPLY_ANCHOR.to_string()
        },
    );
    Source {
        lib: pristine.lib,
        property,
        cascade,
    }
}

/// **The proof.** The pristine crate builds; the crate with a property nothing
/// consumes does not.
///
/// The first half is not ceremony. Without it a harness that wrote a broken
/// copy of the source — a missing module, a bad path, a rustc that is not
/// there — would report every injection below as "the build failed", which is
/// the answer they are asserting, and the whole file would pass while proving
/// nothing at all.
#[test]
fn the_pristine_crate_builds_and_a_property_with_no_consumer_does_not() {
    let (ok, stderr) = Source::pristine().compile("pristine");
    assert!(
        ok,
        "the unmodified crate did not compile, so nothing below proves anything:\n{stderr}"
    );

    let (ok, stderr) = with_variant(false, false, false).compile("no-consumer");
    assert!(
        !ok,
        "a property with no consumer at all compiled, which is decision 5 not holding"
    );
    assert!(
        stderr.contains("E0004"),
        "the build failed for some other reason than a non-exhaustive match:\n{stderr}"
    );
    assert!(
        stderr.contains("BorderCollapse"),
        "the error does not name the variant that was added:\n{stderr}"
    );
}

/// The consumer the plan names: a property parsed and never written into a
/// computed style.
///
/// Both other arms are supplied, so the only thing missing is the one that
/// *does* something with the property — which is exactly the state a
/// half-finished property lands in, and exactly the state that produces a page
/// laid out slightly differently with nothing anywhere saying so.
#[test]
fn a_property_that_no_computed_style_consumes_does_not_build() {
    let (ok, stderr) = with_variant(false, true, true).compile("no-apply");
    assert!(
        !ok,
        "a property with a name and an inheritance rule and no consumer compiled"
    );
    assert!(stderr.contains("E0004"), "{stderr}");
    assert!(
        stderr.contains("cascade.rs"),
        "the error should point at `cascade::apply`, the consumer:\n{stderr}"
    );
}

/// The second consumer: a property that is applied and then anonymous.
///
/// It is injected separately because one `match` is one consequence. A build
/// with an `apply` arm and no `name` arm lays the property out correctly and
/// cannot report it — so every warning, and the `Unsupported` census the whole
/// gap is judged on, would be silently short by one property.
#[test]
fn a_property_with_no_name_does_not_build() {
    let (ok, stderr) = with_variant(true, false, true).compile("no-name");
    assert!(!ok, "a property with no name compiled");
    assert!(stderr.contains("E0004"), "{stderr}");
    assert!(
        stderr.contains("property.rs"),
        "the error should point at `Property::name`:\n{stderr}"
    );
}

/// The third consumer: a property whose inheritance is nobody's decision.
///
/// `css-cascade-5` §7.2 makes inheritance per-property, and a property that
/// neither inherits nor does not is the quietest wrong answer of the three —
/// it is right on the element that sets it and wrong on every descendant.
#[test]
fn a_property_with_no_inheritance_rule_does_not_build() {
    let (ok, stderr) = with_variant(true, true, false).compile("no-inherited");
    assert!(!ok, "a property with no inheritance rule compiled");
    assert!(stderr.contains("E0004"), "{stderr}");
    assert!(
        stderr.contains("property.rs"),
        "the error should point at `Property::inherited`:\n{stderr}"
    );
}

/// And the other direction: with **all three** arms supplied, the same variant
/// builds.
///
/// This is what says the three tests above fail because of the missing arm and
/// not because adding any variant at all breaks the crate — which would make
/// them true and meaningless. It is the assertion that cannot be satisfied by
/// a harness that simply never compiles anything.
#[test]
fn the_same_variant_with_every_arm_supplied_builds() {
    let (ok, stderr) = with_variant(true, true, true).compile("all-arms");
    assert!(
        ok,
        "a fully consumed property did not build, so the proofs above prove only that the \
         injection breaks something:\n{stderr}"
    );
}
