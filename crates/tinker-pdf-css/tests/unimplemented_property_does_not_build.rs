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
//! # The four consumers of a property, injected separately
//!
//! One `match` is one consequence, and gap 31's own rule is that a test for one
//! of two independent consequences is not a test. [`Property`] has four
//! exhaustive consumers and each is withheld on its own:
//!
//! - `cascade::apply`, which is the one the plan names — a property that is
//!   parsed and never written into a computed style;
//! - `Property::name`, without which a property would be applied and then
//!   anonymous in every warning and every census;
//! - `Property::inherited`, without which it would be applied, named, and
//!   inherit or not by accident;
//! - `Property::longhand`, without which no `css-cascade-5` §7.1 defaulting
//!   keyword could name it — `color: inherit` would work and the new
//!   property's `inherit` would silently do nothing.
//!
//! # What this file did **not** cover until §7.1 landed, and now does
//!
//! Everything above is about a **property**. For most of this file's life it
//! said nothing whatever about a **value** that no consumer reads, and the
//! distinction is not academic: the five defaulting keywords are values, valid
//! on every property, and a build could have grown one of them with nothing to
//! write it down and no `match` anywhere to notice.
//!
//! That is what [`Longhand`] and its three exhaustive consumers are for, and
//! why they are injected here too:
//!
//! - `Longhand::name`, `Longhand::inherited` — a property name a keyword can
//!   be written on and whose inheritance `unset` cannot ask about;
//! - `cascade::copy_computed`, the one that actually moves a value. A longhand
//!   missing that arm is `inherit` parsing, cascading, winning, and writing
//!   nothing.
//!
//! `Longhand::ALL` is the one thing here `rustc` cannot check, because it is a
//! list and not a `match`. `crates/tinker-pdf-css/src/tests/defaulting.rs`
//! checks it against `IMPLEMENTED_NAMES` instead, and this file does not
//! pretend otherwise.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the anchors live, and what each one is for.
const APPLY_ANCHOR: &str = "// <<< the compile-time proof's fourth arm goes here >>>";
const NAME_ANCHOR: &str = "// <<< the compile-time proof's second arm goes here >>>";
const INHERITED_ANCHOR: &str = "// <<< the compile-time proof's third arm goes here >>>";
const VARIANT_ANCHOR: &str =
    "// <<< the compile-time proof injects a variant directly above this line >>>";
const LONGHAND_ANCHOR: &str = "// <<< the compile-time proof's fifth arm goes here >>>";
const LONGHAND_NAME_ANCHOR: &str = "// <<< the compile-time proof's sixth arm goes here >>>";
const LONGHAND_INHERITED_ANCHOR: &str = "// <<< the compile-time proof's seventh arm goes here >>>";
const COPY_ANCHOR: &str = "// <<< the compile-time proof's eighth arm goes here >>>";
const LONGHAND_VARIANT_ANCHOR: &str =
    "// <<< the compile-time proof injects a longhand directly above this line >>>";

/// The `Longhand` the injected property sets, and its own three arms.
const LONGHAND_VARIANT: &str = "    TextTransform,";
const LONGHAND_ARM: &str = "            Property::TextTransform(_) => Longhand::TextTransform,";
const LONGHAND_NAME_ARM: &str = "            Longhand::TextTransform => \"text-transform\",";
const LONGHAND_INHERITED_ARM: &str = "            Longhand::TextTransform => true,";
const COPY_ARM: &str = "        Longhand::TextTransform => into.color = from.color.clone(),";

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
/// `border-collapse` was the successor and milestone 11 implemented it, on
/// exactly the schedule the paragraph above predicted — *"when that milestone
/// lands this constant moves again"*. `vertical-align` was the third and tier
/// 4 implemented it, in the same commit as this line. It has now moved three
/// times, which is the standing evidence that the choice is the right kind: a
/// name that never had to move would be a name nobody was ever going to
/// implement, and a proof injecting one would be asserting something about a
/// property this build had decided never to have.
///
/// `text-transform` is the successor and it is chosen the same way, on the
/// three tests the moves above have settled between them.
///
/// **It is in `UNSUPPORTED_PROPERTIES` and stays there.** Tier 4 consumes
/// `content` and `gap`, so neither could have been picked without this constant
/// moving again inside the same tier.
///
/// **It is genuinely unimplemented, for a reason that can be stated.**
/// `css-text-4` §2.1's `uppercase` and `lowercase` are not a `char::to_uppercase`
/// away: the mapping is locale-dependent (Turkish `i` uppercases to `İ` and
/// dotless `ı` lowercases from `I`), context-dependent (a Greek final sigma is
/// `ς` at the end of a word and `σ` inside it), and not length-preserving (`ß`
/// uppercases to `SS`, so a transformed run measures differently from the one
/// the source wrote). All three need Unicode's `SpecialCasing.txt`, which is
/// **not** among the files vendored at `crates/tinker-pdf-layout/data/ucd` —
/// that directory holds `DerivedGeneralCategory.txt`, `EastAsianWidth.txt`,
/// `LineBreak.txt`, `LineBreakTest.txt` and `emoji-data.txt`, all of them
/// UAX #14's. A build that reached for the ASCII answer would set a Turkish
/// book's headings wrong and nothing would look broken.
///
/// **And it is a name somebody will implement one day**, which is the test the
/// three moves above have made the important one. `writing-mode` would have
/// been the easy wrong answer here: it is unimplemented, it is in the same
/// list, and it is refused *permanently* by the vertical-text non-goal in
/// `docs/design/shaping.md` — so it would never move, and a constant that never
/// moves is one nobody ever re-reads.
const VARIANT: &str = "    TextTransform(bool),";
const APPLY_ARM: &str = "        Property::TextTransform(_) => {}";
const NAME_ARM: &str = "            Property::TextTransform(_) => \"text-transform\",";
const INHERITED_ARM: &str = "            Property::TextTransform(_) => true,";

struct Source {
    lib: String,
    property: String,
    cascade: String,
    longhand: String,
}

impl Source {
    /// The crate's own source, read from the manifest directory.
    fn pristine() -> Self {
        let root = crate_root();
        Self {
            lib: read(&root.join("src/lib.rs")),
            property: read(&root.join("src/property.rs")),
            cascade: read(&root.join("src/cascade.rs")),
            longhand: read(&root.join("src/longhand.rs")),
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
        std::fs::write(src.join("longhand.rs"), &self.longhand).expect("write");

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

/// A variant added, with the arms named supplied and the rest not.
///
/// The `Longhand` side is always supplied here: a `Property` variant needs a
/// `Longhand` to map onto, so withholding both at once would fail for two
/// reasons and prove neither. [`with_longhand`] withholds that side on its own.
fn with_variant(
    supply_apply: bool,
    supply_name: bool,
    supply_inherited: bool,
    supply_longhand: bool,
) -> Source {
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
    let mut cascade = inject(
        &pristine.cascade,
        APPLY_ANCHOR,
        &if supply_apply {
            format!("{APPLY_ARM}\n        {APPLY_ANCHOR}")
        } else {
            APPLY_ANCHOR.to_string()
        },
    );
    cascade = inject(
        &cascade,
        COPY_ANCHOR,
        &format!("{COPY_ARM}\n        {COPY_ANCHOR}"),
    );
    let longhand = with_longhand_arms(&pristine.longhand, supply_longhand, true, true);
    Source {
        lib: pristine.lib,
        property,
        cascade,
        longhand,
    }
}

/// The `Longhand` variant added, with each of its own arms supplied or not.
fn with_longhand_arms(
    source: &str,
    supply_longhand: bool,
    supply_name: bool,
    supply_inherited: bool,
) -> String {
    let mut longhand = inject(
        source,
        LONGHAND_VARIANT_ANCHOR,
        &format!("{LONGHAND_VARIANT}\n    {LONGHAND_VARIANT_ANCHOR}"),
    );
    longhand = inject(
        &longhand,
        LONGHAND_ANCHOR,
        &if supply_longhand {
            format!("{LONGHAND_ARM}\n            {LONGHAND_ANCHOR}")
        } else {
            LONGHAND_ANCHOR.to_string()
        },
    );
    longhand = inject(
        &longhand,
        LONGHAND_NAME_ANCHOR,
        &if supply_name {
            format!("{LONGHAND_NAME_ARM}\n            {LONGHAND_NAME_ANCHOR}")
        } else {
            LONGHAND_NAME_ANCHOR.to_string()
        },
    );
    inject(
        &longhand,
        LONGHAND_INHERITED_ANCHOR,
        &if supply_inherited {
            format!("{LONGHAND_INHERITED_ARM}\n            {LONGHAND_INHERITED_ANCHOR}")
        } else {
            LONGHAND_INHERITED_ANCHOR.to_string()
        },
    )
}

/// A **longhand** added with no `Property` behind it, and one of its consumers
/// withheld.
///
/// The value-side proof. Nothing about `Property` changes, so a failure here is
/// about a property *name* that §7.1's keywords can be written on and that the
/// cascade cannot write down.
fn with_longhand(supply_copy: bool, supply_name: bool, supply_inherited: bool) -> Source {
    let pristine = Source::pristine();
    let cascade = inject(
        &pristine.cascade,
        COPY_ANCHOR,
        &if supply_copy {
            format!("{COPY_ARM}\n        {COPY_ANCHOR}")
        } else {
            COPY_ANCHOR.to_string()
        },
    );
    // No `Property::longhand` arm, because no `Property` variant was added.
    let longhand = with_longhand_arms(&pristine.longhand, true, supply_name, supply_inherited);
    let longhand = longhand.replace(
        &format!("{LONGHAND_ARM}\n            {LONGHAND_ANCHOR}"),
        LONGHAND_ANCHOR,
    );
    Source {
        lib: pristine.lib,
        property: pristine.property,
        cascade,
        longhand,
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

    let (ok, stderr) = with_variant(false, false, false, false).compile("no-consumer");
    assert!(
        !ok,
        "a property with no consumer at all compiled, which is decision 5 not holding"
    );
    assert!(
        stderr.contains("E0004"),
        "the build failed for some other reason than a non-exhaustive match:\n{stderr}"
    );
    assert!(
        stderr.contains("TextTransform"),
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
    let (ok, stderr) = with_variant(false, true, true, true).compile("no-apply");
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
    let (ok, stderr) = with_variant(true, false, true, true).compile("no-name");
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
    let (ok, stderr) = with_variant(true, true, false, true).compile("no-inherited");
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
    let (ok, stderr) = with_variant(true, true, true, true).compile("all-arms");
    assert!(
        ok,
        "a fully consumed property did not build, so the proofs above prove only that the \
         injection breaks something:\n{stderr}"
    );
}

/// The fourth consumer of a property: one that no defaulting keyword can name.
///
/// `Property::longhand` is what turns a property into something §7.1 can be
/// written on. Withhold it and the property parses, cascades and lays out —
/// and `inherit` on it is a declaration the cascade cannot even address.
#[test]
fn a_property_no_defaulting_keyword_can_name_does_not_build() {
    let (ok, stderr) = with_variant(true, true, true, false).compile("no-longhand");
    assert!(
        !ok,
        "a property that no defaulting keyword can name compiled"
    );
    assert!(stderr.contains("E0004"), "{stderr}");
    assert!(
        stderr.contains("longhand.rs"),
        "the error should point at `Property::longhand`:\n{stderr}"
    );
}

/// **The value-side proof**, and the one this file could not make until §7.1
/// landed: a property *name* the cascade cannot write a defaulted value into.
///
/// Nothing about `Property` changes here. A `Longhand` is added — which is what
/// a new property name is — and `cascade::copy_computed` is withheld. The
/// result is `inherit` on that name parsing, cascading, winning, and writing
/// nothing at all: the quietest of the failures this file exists to prevent,
/// because the page renders and the property simply keeps whatever it had.
#[test]
fn a_longhand_the_cascade_cannot_write_does_not_build() {
    let (ok, stderr) = with_longhand(false, true, true).compile("no-copy");
    assert!(
        !ok,
        "a longhand that no defaulting keyword can write compiled"
    );
    assert!(stderr.contains("E0004"), "{stderr}");
    assert!(
        stderr.contains("cascade.rs"),
        "the error should point at `copy_computed`:\n{stderr}"
    );
}

/// A longhand with no name, which is the census failure one level down: a
/// property name that cannot be printed cannot be matched by `from_name`
/// either, so no stylesheet could ever address it.
#[test]
fn a_longhand_with_no_name_does_not_build() {
    let (ok, stderr) = with_longhand(true, false, true).compile("no-longhand-name");
    assert!(!ok, "a longhand with no name compiled");
    assert!(stderr.contains("E0004"), "{stderr}");
    assert!(stderr.contains("longhand.rs"), "{stderr}");
}

/// A longhand whose inheritance nobody decided, which is exactly the value
/// `unset` asks for: §7.1 defines it as `inherit` or `initial` *according to
/// this answer*, so a missing one is `unset` doing something arbitrary.
#[test]
fn a_longhand_with_no_inheritance_rule_does_not_build() {
    let (ok, stderr) = with_longhand(true, true, false).compile("no-longhand-inherited");
    assert!(!ok, "a longhand with no inheritance rule compiled");
    assert!(stderr.contains("E0004"), "{stderr}");
    assert!(stderr.contains("longhand.rs"), "{stderr}");
}

/// And the other direction for the value side: a fully consumed longhand
/// builds, so the three above fail for the arm and not for the variant.
#[test]
fn the_same_longhand_with_every_arm_supplied_builds() {
    let (ok, stderr) = with_longhand(true, true, true).compile("longhand-all-arms");
    assert!(
        ok,
        "a fully consumed longhand did not build, so the proofs above prove only that adding \
         one breaks something:\n{stderr}"
    );
}
