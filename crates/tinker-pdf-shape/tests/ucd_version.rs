//! The two vendored UCD trees are the same Unicode version.
//!
//! `docs/design/shaping.md`'s risk table names the failure this exists to stop:
//! *"Unicode version skew between the two crates' vendored UCD — one pinned
//! version, asserted by a header-comparison test; upgrades touch both
//! `data/ucd` trees in one commit."*
//!
//! # Why skew is worse than being out of date
//!
//! `tinker-pdf-layout` breaks lines with UAX #14 and `tinker-pdf-shape`
//! resolves direction with UAX #9, and a paragraph goes through both. Two
//! versions means two answers about the same character: one crate knows a code
//! point is assigned and the other does not, one gives it a Line_Break class
//! and the other `XX`. Nothing fails; the line simply breaks in a place the
//! other half of the engine would not have chosen. Being a version behind is
//! visible and fixable. Being *half* a version behind is neither.
//!
//! The files themselves say which version they are, on their first line, so
//! this reads the pin out of the data rather than out of a constant somebody
//! has to remember to bump.

use std::path::{Path, PathBuf};

/// The vendored trees, and one file from each that carries a version header.
///
/// Every file in both trees is checked, not only these; the list is here to
/// say where the trees are.
const TREES: &[&str] = &[
    "crates/tinker-pdf-shape/data/ucd",
    "crates/tinker-pdf-layout/data/ucd",
];

fn repo_root() -> PathBuf {
    // `crates/tinker-pdf-shape` -> the repository.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate is two levels below the repository root")
        .to_path_buf()
}

/// The version a UCD file states, in either of the two ways it may.
///
/// Most files put it in the first line's own filename — `# LineBreak-17.0.0.txt`
/// — and the UTS #51 emoji files put it on a `# Version: 17.0` line instead,
/// to two components rather than three. Both are read, and the shorter one is
/// compared as a prefix, because it is genuinely the same release stated less
/// precisely and not a different one.
///
/// `None` for a file with neither, which is `LICENSE.txt` and nothing else —
/// asserted below, so a data file that lost its header is a failure rather
/// than a file quietly not compared.
fn version(text: &str) -> Option<String> {
    let dotted = |value: &str| {
        (!value.is_empty()
            && value
                .split('.')
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())))
        .then(|| value.to_string())
    };
    let first = text.lines().next()?.trim_end_matches('\r');
    if let Some(version) = first
        .strip_prefix("# ")
        .and_then(|stem| stem.strip_suffix(".txt"))
        .and_then(|stem| stem.rsplit_once('-'))
        .and_then(|(_, version)| dotted(version))
    {
        return Some(version);
    }
    text.lines()
        .take(20)
        .find_map(|line| line.trim_end_matches('\r').strip_prefix("# Version: "))
        .and_then(|value| dotted(value.trim()))
}

/// Whether two stated versions are the same release.
///
/// Equal, or one a dotted prefix of the other: `17.0` and `17.0.0` are the
/// same Unicode release written to different precision, while `17.0` and
/// `17.1` are not, and neither are `17.0.0` and `17.0.1`.
fn same_release(a: &str, b: &str) -> bool {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    long == short || long.starts_with(&format!("{short}."))
}

/// Every `.txt` file of a tree, with the version its header states.
fn tree(root: &Path, path: &str) -> Vec<(String, Option<String>)> {
    let directory = root.join(path);
    let entries = std::fs::read_dir(&directory).unwrap_or_else(|error| {
        panic!(
            "{} could not be read ({error}); the version-skew test cannot be \
             skipped, because a skipped oracle exits 0 and reads exactly like \
             a pass",
            directory.display()
        )
    });
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".txt") {
            continue;
        }
        // Only the first line is needed and two of these files are eight
        // megabytes, so the whole file is not read.
        let text = std::fs::read(entry.path()).expect("a vendored file");
        let head = String::from_utf8_lossy(&text[..text.len().min(512)]).to_string();
        out.push((name, version(&head)));
    }
    out.sort();
    assert!(!out.is_empty(), "{path} holds no vendored data at all");
    out
}

#[test]
fn both_crates_vendor_the_same_unicode_version() {
    let root = repo_root();
    let mut versions: Vec<(String, String, String)> = Vec::new();
    for path in TREES {
        for (name, version) in tree(&root, path) {
            match version {
                Some(version) => versions.push(((*path).to_string(), name, version)),
                // Two files carry no version header, and each is pinned
                // another way: the licence is compared byte for byte between
                // the trees, and `UnicodeData.txt` -- the one UCD data file
                // whose first line is data rather than a header -- by the
                // repertoire cross-check below.
                None => assert!(
                    UNVERSIONED.contains(&name.as_str()),
                    "{path}/{name} states no Unicode version in its header, so \
                     nothing here can tell whether it drifted"
                ),
            }
        }
    }
    assert!(
        versions.len() >= 12,
        "only {} versioned files across both trees",
        versions.len()
    );
    // The longest stated version is the pin, so the two-component emoji header
    // is compared against the three-component one rather than the other way
    // round.
    let pinned = versions
        .iter()
        .map(|(_, _, version)| version.clone())
        .max_by_key(String::len)
        .expect("at least one versioned file");
    let disagree: Vec<String> = versions
        .iter()
        .filter(|(_, _, version)| !same_release(version, &pinned))
        .map(|(path, name, version)| format!("{path}/{name} is {version}, not {pinned}"))
        .collect();
    assert!(
        disagree.is_empty(),
        "the two vendored UCD trees have drifted apart:\n{}",
        disagree.join("\n")
    );
    // And the pin is the one THIRDPARTY.md declares, so the two statements of
    // it cannot disagree either.
    let manifest = std::fs::read_to_string(root.join("THIRDPARTY.md")).expect("THIRDPARTY.md");
    assert!(
        manifest.contains(&format!("version {pinned}")),
        "THIRDPARTY.md does not declare the Unicode version the data is at ({pinned})"
    );
}

#[test]
fn both_crates_carry_the_same_licence_text() {
    let root = repo_root();
    let read = |path: &str| {
        std::fs::read_to_string(root.join(path).join("LICENSE.txt"))
            .unwrap_or_else(|error| panic!("{path}/LICENSE.txt could not be read ({error})"))
    };
    assert_eq!(
        read(TREES[0]).replace("\r\n", "\n"),
        read(TREES[1]).replace("\r\n", "\n"),
        "the two vendored UCD trees carry different licence text, so one of \
         them is describing terms it was not given under"
    );
}

/// The files each tree is supposed to hold, so that a re-vendor that dropped
/// one is a failure rather than a table that silently answers `None`.
#[test]
fn each_tree_holds_the_files_its_algorithms_need() {
    let root = repo_root();
    let names = |path: &str| -> Vec<String> {
        tree(&root, path)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    };
    assert_eq!(
        names("crates/tinker-pdf-shape/data/ucd"),
        vec![
            "BidiBrackets.txt",
            "BidiCharacterTest.txt",
            "BidiMirroring.txt",
            "BidiTest.txt",
            "DerivedBidiClass.txt",
            "DerivedJoiningType.txt",
            "IndicPositionalCategory.txt",
            "IndicSyllabicCategory.txt",
            "LICENSE.txt",
            "PropertyValueAliases.txt",
            "Scripts.txt",
            "UnicodeData.txt",
        ],
        "the shaping crate's vendored UCD changed shape"
    );
    assert_eq!(
        names("crates/tinker-pdf-layout/data/ucd"),
        vec![
            "DerivedGeneralCategory.txt",
            "EastAsianWidth.txt",
            "LICENSE.txt",
            "LineBreak.txt",
            "LineBreakTest.txt",
            "emoji-data.txt",
        ],
        "the layout crate's vendored UCD changed shape"
    );
}

/// The vendored files that state no Unicode version of their own.
///
/// Two, and each is pinned by something other than a header:
///
/// - `LICENSE.txt`, compared byte for byte between the two trees by
///   [`both_crates_carry_the_same_licence_text`];
/// - `UnicodeData.txt`, whose very first line is data — it is the one UCD data
///   file published without a header — pinned by
///   [`unicode_data_covers_the_repertoire_the_versioned_files_name`].
const UNVERSIONED: &[&str] = &["LICENSE.txt", "UnicodeData.txt"];

/// Every code point `Scripts.txt` names is one `UnicodeData.txt` knows about.
///
/// # What this pins, and what it does not
///
/// `UnicodeData.txt` carries no version header, so the comparison every other
/// file gets is unavailable for it. What is available is its **repertoire**:
/// UAX #24 gives every assigned character a script, so every code point
/// `Scripts.txt` lists must appear in `UnicodeData.txt` — directly, or inside
/// one of its `First>`/`Last>` ranges. A `UnicodeData.txt` from an older
/// release is missing the characters the release after it assigned, and this
/// finds them by name.
///
/// It does **not** catch a `UnicodeData.txt` newer than the rest of the tree:
/// a superset still contains everything. That direction is left to the upgrade
/// discipline `docs/design/shaping.md` states — a version bump touches every
/// file in both trees in one commit — and is written down here rather than
/// implied, because a check that half works is worse than one that says which
/// half.
///
/// The converse containment is deliberately not asserted either:
/// `UnicodeData.txt` lists surrogates and private-use blocks that `Scripts.txt`
/// leaves `Unknown` and therefore does not name, so the two sets are not equal
/// and never were.
#[test]
fn unicode_data_covers_the_repertoire_the_versioned_files_name() {
    let root = repo_root();
    let ucd = root.join(TREES[0]);
    let data = std::fs::read_to_string(ucd.join("UnicodeData.txt")).expect("UnicodeData.txt");

    // The code points `UnicodeData.txt` accounts for, with its ranges opened
    // out. A `<..., First>` line and the `<..., Last>` line after it stand for
    // everything between, which is how the CJK and Hangul blocks are written.
    let mut known: Vec<(u32, u32)> = Vec::new();
    let mut first: Option<u32> = None;
    for line in data.lines() {
        let mut fields = line.split(';');
        let Some(code) = fields.next().and_then(|c| u32::from_str_radix(c, 16).ok()) else {
            continue;
        };
        let name = fields.next().unwrap_or_default();
        if name.ends_with(", First>") {
            first = Some(code);
            continue;
        }
        if let (Some(start), true) = (first, name.ends_with(", Last>")) {
            known.push((start, code));
            first = None;
            continue;
        }
        known.push((code, code));
    }
    assert!(
        known.len() > 30_000,
        "UnicodeData.txt yielded only {} rows, so it is not the file it \
         claims to be",
        known.len()
    );
    known.sort_unstable();
    let covers = |code: u32| -> bool {
        known
            .binary_search_by(|(low, high)| {
                if code < *low {
                    core::cmp::Ordering::Greater
                } else if code > *high {
                    core::cmp::Ordering::Less
                } else {
                    core::cmp::Ordering::Equal
                }
            })
            .is_ok()
    };

    let scripts = std::fs::read_to_string(ucd.join("Scripts.txt")).expect("Scripts.txt");
    let mut named = 0usize;
    let mut missing: Vec<u32> = Vec::new();
    for line in scripts.lines() {
        let body = line.split('#').next().unwrap_or_default().trim();
        let Some(codes) = body.split(';').next().map(str::trim) else {
            continue;
        };
        if codes.is_empty() {
            continue;
        }
        let (low, high) = match codes.split_once("..") {
            Some((a, b)) => (
                u32::from_str_radix(a, 16).unwrap_or(0),
                u32::from_str_radix(b, 16).unwrap_or(0),
            ),
            None => {
                let one = u32::from_str_radix(codes, 16).unwrap_or(0);
                (one, one)
            }
        };
        for code in low..=high {
            named += 1;
            if !covers(code) && missing.len() < 8 {
                missing.push(code);
            }
        }
    }
    assert!(
        named > 100_000,
        "Scripts.txt named only {named} code points"
    );
    assert!(
        missing.is_empty(),
        "UnicodeData.txt does not know about {missing:04X?}, which Scripts.txt \
         names — so it is from an earlier Unicode release than the rest of the \
         tree"
    );
}
