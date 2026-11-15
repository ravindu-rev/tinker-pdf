//! Compiles the vendored hyphenation dictionaries, under the `hyphenation`
//! feature.
//!
//! The Unicode Character Database tables line breaking and casing read are
//! `tinker-pdf-unicode`'s, compiled by that crate's own `build.rs`.
//!
//! Each vendored pattern file (`data/hyphenation-<tag>/hyph-<tag>.tex`,
//! THIRDPARTY.md) is read and compiled by `src/hyphenate/compile.rs` —
//! included below by path, so the reader is the one `Dictionary::parse` runs
//! at run time — into its four arrays, which are written to `OUT_DIR` and
//! included by `hyphenation.rs`'s registry with `include_bytes!`. A file the
//! reader refuses, or one whose header states no typesetting minimums, fails
//! the build by name. Without the feature nothing is compiled or written, so
//! the default build reads no dictionary and carries none.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The pattern-file reader and trie builder the crate's `hyphenate` module
/// uses, by path. Not every item it has is needed here.
#[allow(dead_code)]
#[path = "src/hyphenate/compile.rs"]
mod hyphenation_compile;

/// The vendored dictionaries: the tag each tree and file is named by, and the
/// language tag the table is found by (`docs/design/hyphenation.md`).
const DICTIONARIES: [(&str, &str); 8] = [
    ("af", "af"),
    ("cu", "cu"),
    ("en-gb", "en-GB"),
    ("ga", "ga"),
    ("nl", "nl"),
    ("pl", "pl"),
    ("pt", "pt"),
    ("sk", "sk"),
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Where `tinker-pdf-unicode`'s build script wrote its tables, for
    // `tests/uncascaded_field_does_not_build.rs`, which compiles that crate's
    // real modules against them before this crate's `style.rs`.
    if let Ok(tables) = std::env::var("DEP_TINKER_PDF_UNICODE_OUT_DIR") {
        println!("cargo:rustc-env=TINKER_PDF_UNICODE_OUT_DIR={tables}");
    }
    println!("cargo:rerun-if-changed=src/hyphenate/compile.rs");
    if std::env::var_os("CARGO_FEATURE_HYPHENATION").is_some() {
        hyphenation(
            &PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets this")),
            &PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets this")),
        );
    }
}

/// Compiles every vendored hyphenation dictionary into `OUT_DIR`: four array
/// files each, and `hyphenation.rs`, the registry `crate::hyphenate`
/// includes.
///
/// The arrays go out as files and come back with `include_bytes!` rather
/// than as array literals in the registry: the eight tables are about a
/// hundred thousand nodes, and a literal of that size is compile time spent
/// on every build of the feature for nothing a byte string does not do.
fn hyphenation(manifest: &Path, out_dir: &Path) {
    let mut statics = String::new();
    let mut tables = String::new();
    for (name, serves) in DICTIONARIES {
        let relative = format!("data/hyphenation-{name}/hyph-{name}.tex");
        println!("cargo:rerun-if-changed={relative}");
        let path = manifest.join(&relative);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "the vendored dictionary is missing: {} ({e})",
                path.display()
            )
        });
        let source = hyphenation_compile::read(&text)
            .unwrap_or_else(|e| panic!("{relative} does not read: {e}"));
        let minimums = source
            .minimums
            .unwrap_or_else(|| panic!("{relative} states no typesetting minimums"));
        let compiled = hyphenation_compile::compile(&source, minimums)
            .unwrap_or_else(|e| panic!("{relative} does not compile: {e}"));
        for (part, bytes) in [
            ("alphabet", &compiled.alphabet),
            ("nodes", &compiled.nodes),
            ("offsets", &compiled.offsets),
            ("values", &compiled.values),
        ] {
            let file = out_dir.join(format!("hyph-{name}.{part}"));
            std::fs::write(&file, bytes)
                .unwrap_or_else(|e| panic!("{} could not be written ({e})", file.display()));
        }
        let constant = name.replace('-', "_").to_ascii_uppercase();
        let array = |part: &str| {
            format!("include_bytes!(concat!(env!(\"OUT_DIR\"), \"/hyph-{name}.{part}\"))")
        };
        let _ = writeln!(
            statics,
            "/// `{relative}`: {} letters, {} patterns, {} published words.\n\
             static {constant}: Dictionary = Dictionary::from_static(\n    ({}, {}, {}, {}),\n    \
             {},\n    ({}, {}),\n);",
            compiled.alphabet.len() / 4,
            source.patterns.len(),
            source.exceptions.len(),
            array("alphabet"),
            array("nodes"),
            array("offsets"),
            array("values"),
            compiled.longest,
            compiled.left,
            compiled.right,
        );
        let _ = writeln!(
            tables,
            "    Table {{\n        name: \"{name}\",\n        language: \"{serves}\",\n        \
             dictionary: &{constant},\n    }},"
        );
    }
    let registry = format!(
        "// Generated by build.rs from the vendored hyphenation dictionaries. Do not edit.\n\
         {statics}\n\
         /// Every table this build carries, in the order the design lists them.\n\
         static TABLES: &[Table] = &[\n{tables}];\n"
    );
    let target = out_dir.join("hyphenation.rs");
    std::fs::write(&target, registry)
        .unwrap_or_else(|e| panic!("{} could not be written ({e})", target.display()));
}
