//! Every enum number a binding carries is the header's, for every arm.
//!
//! The bindings over the C ABI name each enum's numbers themselves: Ruby's
//! modules and .NET's enums write them out, Java's enums are passed by
//! `ordinal()`, so their declaration order *is* the number, and Go and Swift
//! name the header's own constants. A wrong number for an arm sends a
//! different arm across -- Ruby's `SoftMask::ABSENT = 1` writes `/SMask
//! /None` on every graphics state without an explicit mask -- and still
//! compiles, still runs, and passes `cargo xtask bindings-parity` whenever the
//! parity scripts never use that arm, which for most arms they do not (review
//! of lane 7C). The C crate's own tests pin the Rust numbers; nothing pinned
//! the bindings' copies of them.
//!
//! So this reads the committed header, which cbindgen generates from the Rust
//! enums (`cbindgen.toml`; CI regenerates and diffs it), and every binding's
//! source as text, and holds each enum a binding carries to the header's arms:
//! the same arms, none missing and none extra, each with the header's number.
//! Arms are matched by name with case and underscores ignored, so
//! `TPDF_SOFT_MASK_ABSENT`, `SoftMask::ABSENT`, `SoftMask.Absent`,
//! `SoftMask.ABSENT` and `SoftMaskAbsent` are one arm. It spawns nothing
//! (ruling 13; `cargo xtask oracles`).
//!
//! Python and JavaScript carry no numbers -- they are Rust over the facade and
//! name each arm by a string -- so what is held there is that each string is
//! its own arm's name (`python_and_javascript_name_each_arm_after_itself`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// An enum's arms: normalised arm name to number, in declaration order.
type Arms = Vec<(String, i64)>;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Lower case with every underscore removed: the one spelling all five
/// languages' names share.
fn norm(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// Every source file under `dir` with extension `ext`, recursively, sorted.
fn sources(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            sources(&path, ext, out);
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

fn all_sources(dir: &str, ext: &str) -> Vec<(PathBuf, String)> {
    let mut paths = Vec::new();
    sources(&repo().join(dir), ext, &mut paths);
    assert!(!paths.is_empty(), "no .{ext} under {dir}");
    paths
        .into_iter()
        .map(|path| {
            let text = read(&path);
            (path, text)
        })
        .collect()
}

/// The text with every `//` comment removed, which also removes `///` and
/// C#'s XML documentation.
fn without_line_comments(text: &str) -> String {
    text.lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The header's enums, by name without `Tpdf` (normalised), with each arm's
/// name stripped of its `TPDF_<ENUM>_` prefix.
fn header() -> BTreeMap<String, (String, Arms)> {
    let text = without_line_comments(&read(
        &repo().join("crates/tinker-pdf-ffi/include/tinker_pdf.h"),
    ));
    let mut enums = BTreeMap::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("typedef enum ") {
        rest = &rest[start + "typedef enum ".len()..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let open = rest.find('{').expect("an enum body");
        let close = rest.find('}').expect("an enum body ends");
        let body = &rest[open + 1..close];
        let short = name
            .strip_prefix("Tpdf")
            .expect("every enum is Tpdf-prefixed");
        let prefix = norm(&format!("TPDF_{short}"));
        let mut arms = Vec::new();
        for item in body.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            let (constant, value) = item
                .split_once('=')
                .unwrap_or_else(|| panic!("{name}: every arm states its number: {item:?}"));
            let constant = norm(constant.trim());
            let arm = constant
                .strip_prefix(&prefix)
                .unwrap_or_else(|| panic!("{name}: {constant} does not start {prefix}"));
            let value: i64 = value.trim().parse().expect("a decimal number");
            arms.push((arm.to_string(), value));
        }
        enums.insert(norm(short), (name.clone(), arms));
        rest = &rest[close..];
    }
    assert!(
        enums.len() >= 30,
        "the header scan found {} enums, which means it is not reading the header",
        enums.len()
    );
    enums
}

/// Ruby: `module Name` ... `end`, holding `CONSTANT = number` lines.
fn ruby() -> BTreeMap<String, Arms> {
    let mut found = BTreeMap::new();
    for (_, text) in all_sources("bindings/ruby/lib", "rb") {
        let mut current: Option<(String, Arms)> = None;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if let Some(name) = line.strip_prefix("module ") {
                current = Some((norm(name.trim()), Vec::new()));
            } else if line == "end" {
                if let Some((name, arms)) = current.take() {
                    if !arms.is_empty() {
                        found.insert(name, arms);
                    }
                }
            } else if let Some((_, arms)) = current.as_mut() {
                if let Some((constant, value)) = line.split_once(" = ") {
                    if let Ok(value) = value.trim().parse::<i64>() {
                        arms.push((norm(constant.trim()), value));
                    }
                }
            }
        }
    }
    found
}

/// Java: `enum Name { A, B, C }`; the number is the position, because the
/// binding passes `ordinal()`.
fn java() -> BTreeMap<String, Arms> {
    let mut found = BTreeMap::new();
    for (_, text) in all_sources("bindings/java/src", "java") {
        let text = without_line_comments(&text);
        let mut rest = text.as_str();
        while let Some(start) = rest.find(" enum ") {
            rest = &rest[start + " enum ".len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let (Some(open), Some(close)) = (rest.find('{'), rest.find('}')) else {
                break;
            };
            let body = &rest[open + 1..close];
            assert!(
                !body.contains('(') && !body.contains(';'),
                "Java enum {name} carries more than its constants; this reader \
                 takes each constant's number to be its position"
            );
            let arms = body
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .zip(0..)
                .map(|(item, position)| (norm(item), position))
                .collect();
            found.insert(norm(&name), arms);
            rest = &rest[close..];
        }
    }
    found
}

/// C#: `enum Name { A = 0, B = 1 }`, with an unstated number one past the
/// previous one, as C# assigns it.
fn dotnet() -> BTreeMap<String, Arms> {
    let mut found = BTreeMap::new();
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(repo().join("bindings/dotnet")).expect("bindings/dotnet") {
        let path = entry.expect("a directory entry").path();
        if path.extension().is_some_and(|e| e == "cs") {
            paths.push(path);
        }
    }
    paths.sort();
    assert!(!paths.is_empty(), "no .cs under bindings/dotnet");
    for path in paths {
        let text = without_line_comments(&read(&path));
        let mut rest = text.as_str();
        while let Some(start) = rest.find("public enum ") {
            rest = &rest[start + "public enum ".len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let (Some(open), Some(close)) = (rest.find('{'), rest.find('}')) else {
                break;
            };
            let body = &rest[open + 1..close];
            let mut arms: Arms = Vec::new();
            for item in body.split(',').map(str::trim).filter(|i| !i.is_empty()) {
                let (member, value) = match item.split_once('=') {
                    Some((member, value)) => (
                        member.trim(),
                        value.trim().parse::<i64>().unwrap_or_else(|_| {
                            panic!("{}: {name}.{member} is not a number", path.display())
                        }),
                    ),
                    None => (item, arms.last().map_or(0, |(_, v)| v + 1)),
                };
                arms.push((norm(member), value));
            }
            found.insert(norm(&name), arms);
            rest = &rest[close..];
        }
    }
    found
}

/// Go: `Name Type = C.TPDF_...` inside a `const (` block. The value is the
/// header constant it names, so cgo checks the number; what is checked here
/// is that each Go name is wired to its own arm, that every arm has one, and
/// that no Go enum constant is a literal number.
fn go(header: &BTreeMap<String, (String, Arms)>) -> BTreeMap<String, Arms> {
    let mut found: BTreeMap<String, Arms> = BTreeMap::new();
    for (path, text) in all_sources("bindings/go", "go") {
        for line in without_line_comments(&text).lines() {
            let mut words = line.split_whitespace();
            let (Some(constant), Some(kind), Some("="), Some(value), None) = (
                words.next(),
                words.next(),
                words.next(),
                words.next(),
                words.next(),
            ) else {
                continue;
            };
            let Some((header_name, arms)) = header.get(&norm(kind)) else {
                continue;
            };
            let Some(named) = value.strip_prefix("C.") else {
                panic!(
                    "{}: {constant} {kind} = {value} spells a number by hand; \
                     name the header's constant so cgo checks it",
                    path.display()
                );
            };
            let short = header_name.strip_prefix("Tpdf").unwrap_or(header_name);
            let arm = norm(named)
                .strip_prefix(&norm(&format!("TPDF_{short}")))
                .unwrap_or_else(|| {
                    panic!(
                        "{}: {constant} names {named}, which is not a {header_name}",
                        path.display()
                    )
                })
                .to_string();
            // The Go name is a prefix and the arm, `SoftMaskAbsent`; its arm
            // is the longest header arm it ends with, so `ActionGoToR` is
            // `GO_TO_R` and not `GO_TO`.
            let own = arms
                .iter()
                .map(|(name, _)| name)
                .filter(|name| norm(constant).ends_with(name.as_str()))
                .max_by_key(|name| name.len());
            assert_eq!(
                own,
                Some(&arm),
                "{}: {constant} is wired to {named}",
                path.display()
            );
            let value = arms
                .iter()
                .find(|(name, _)| *name == arm)
                .map(|(_, value)| *value)
                .expect("the arm was found above");
            found.entry(norm(kind)).or_default().push((arm, value));
        }
    }
    found
}

/// Compares one binding's enums with the header's, and answers which header
/// enums it does not carry.
fn compare(
    binding: &str,
    header: &BTreeMap<String, (String, Arms)>,
    carried: &BTreeMap<String, Arms>,
    problems: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut absent = BTreeSet::new();
    for (key, (name, arms)) in header {
        let Some(theirs) = carried.get(key) else {
            absent.insert(name.clone());
            continue;
        };
        let ours: BTreeMap<&str, i64> = arms.iter().map(|(a, v)| (a.as_str(), *v)).collect();
        let mut seen = BTreeSet::new();
        for (arm, value) in theirs {
            if !seen.insert(arm.as_str()) {
                problems.push(format!("{binding}: {name} names {arm} twice"));
            }
            match ours.get(arm.as_str()) {
                None => problems.push(format!(
                    "{binding}: {name} has an arm {arm} the header does not declare"
                )),
                Some(number) if number != value => problems.push(format!(
                    "{binding}: {name}.{arm} is {value}, and the header says {number}"
                )),
                Some(_) => {}
            }
        }
        for arm in ours.keys() {
            if !seen.contains(arm) {
                problems.push(format!("{binding}: {name} is missing the arm {arm}"));
            }
        }
    }
    absent
}

/// The header enums a binding does not carry as a table, each for a stated
/// reason; anything else absent is a binding that lost an enum, or this
/// reader failing to see one, and either fails.
///
/// The same two in every binding, both two-armed and both written as a
/// literal 0 or 1 at the one place each crosses: a target's kind where the
/// binding packs a `TpdfTarget` (1 for a URI), and a sanitise report's list
/// where it reads the report (0 removed, 1 deleted). Neither is checked here;
/// both arms of each cross in a parity script -- `build-a-document` links a
/// page and a URI, `sanitise-report` lists removed entries and deleted
/// objects -- so a swapped pair changes a recorded hash.
const NOT_CARRIED: &[(&str, &[&str])] = &[
    ("ruby", &["TpdfSanitiseList", "TpdfTargetKind"]),
    ("java", &["TpdfSanitiseList", "TpdfTargetKind"]),
    ("dotnet", &["TpdfSanitiseList", "TpdfTargetKind"]),
    ("go", &["TpdfSanitiseList", "TpdfTargetKind"]),
];

#[test]
fn every_enum_number_a_binding_carries_is_the_headers() {
    let header = header();
    let mut problems = Vec::new();
    let carried = [
        ("ruby", ruby()),
        ("java", java()),
        ("dotnet", dotnet()),
        ("go", go(&header)),
    ];
    for (binding, enums) in &carried {
        let absent = compare(binding, &header, enums, &mut problems);
        let expected: BTreeSet<String> = NOT_CARRIED
            .iter()
            .find(|(name, _)| name == binding)
            .map(|(_, names)| names.iter().map(|n| (*n).to_string()).collect())
            .unwrap_or_default();
        if absent != expected {
            problems.push(format!(
                "{binding} carries a different set of the header's enums than \
                 NOT_CARRIED says: absent {absent:?}, expected absent {expected:?}"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Java passes `ordinal()`, so an enum Java carries must number its arms from
/// zero without a gap -- which every enum on this ABI does, and which this
/// holds the header to rather than assuming.
#[test]
fn every_header_enum_counts_from_zero_without_a_gap() {
    for (name, arms) in header().values() {
        let numbers: Vec<i64> = arms.iter().map(|(_, v)| *v).collect();
        let expected: Vec<i64> = (0..).take(numbers.len()).collect();
        assert_eq!(numbers, expected, "{name}");
    }
}

/// The readers are held to known answers, so a binding and a header that both
/// scan as empty cannot agree their way to a pass.
#[test]
fn the_readers_find_the_enums_everybody_uses() {
    let header = header();
    assert_eq!(header["softmask"].1.len(), 3);
    assert_eq!(header["status"].1.len(), 18);
    for (binding, enums) in [
        ("ruby", ruby()),
        ("java", java()),
        ("dotnet", dotnet()),
        ("go", go(&header)),
    ] {
        assert!(
            enums.len() >= 25,
            "{binding}: {} enums found, which means the reader is not reading it",
            enums.len()
        );
        assert_eq!(
            enums.get("softmask").map(Vec::len),
            Some(3),
            "{binding} carries SoftMask"
        );
        assert_eq!(
            enums.get("status").map(Vec::len),
            Some(18),
            "{binding} carries every status"
        );
    }
}

/// Swift names the header's constants (`Int(TPDF_STATUS_NO_SUCH_PAGE.rawValue)`)
/// for the six statuses it gives a caller to branch on, so the C importer
/// checks each number; this checks each Swift name is wired to its own
/// constant. Swift is unverified source (no toolchain), which is all the more
/// reason for a check that needs none.
#[test]
fn swift_names_each_status_by_the_headers_own_constant() {
    let text = read(&repo().join("bindings/swift/Sources/TinkerPdf/TinkerPdf.swift"));
    let header = header();
    let mut wired = 0;
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("public static let ") else {
            continue;
        };
        let (name, value) = rest.split_once(" = ").expect("`let name = value`");
        assert!(
            value.starts_with("Int(TPDF_"),
            "Swift's {name} = {value} spells a number by hand"
        );
        let constant = value
            .trim_start_matches("Int(")
            .trim_end_matches(".rawValue)");
        let arm = norm(constant)
            .strip_prefix("tpdfstatus")
            .unwrap_or_else(|| panic!("{constant} is not a TpdfStatus"))
            .to_string();
        assert_eq!(norm(name), arm, "Swift's {name} is wired to {constant}");
        assert!(
            header["status"].1.iter().any(|(a, _)| *a == arm),
            "{constant} is in the header"
        );
        wired += 1;
    }
    assert_eq!(wired, 6, "the six statuses Swift names");
}

/// Python and JavaScript carry no numbers: each is Rust over the facade and
/// names an arm by a string, `"screen" => BlendMode::Screen` one way and
/// `Weakness::Sha1Digest => "sha1-digest"` the other. A swapped pair there
/// is the same defect as a wrong number elsewhere, and the parity scripts
/// reach as few of those arms, so every such line in both bindings is held
/// to one rule: the string is the variant's own name, with case, hyphens,
/// underscores and spaces ignored. The page boundaries are the one family
/// spelled shorter on purpose (`"trim"` for `TrimBox`), and say so here.
#[test]
fn python_and_javascript_name_each_arm_after_itself() {
    let mut checked = 0;
    let mut problems = Vec::new();
    for dir in ["bindings/python/src", "bindings/js/src"] {
        for (path, text) in all_sources(dir, "rs") {
            for line in without_line_comments(&text).lines() {
                let Some((left, right)) = line.trim().split_once(" => ") else {
                    continue;
                };
                let quoted = |side: &str| {
                    let side = side.trim().trim_end_matches(',');
                    side.strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .map(str::to_string)
                };
                // `Enum::Variant`, possibly with a payload pattern after it.
                let variant = |side: &str| {
                    let side = side.trim().trim_end_matches(',');
                    let (kind, rest) = side.split_once("::")?;
                    if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphanumeric()) {
                        return None;
                    }
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric())
                        .collect();
                    let first = name.chars().next()?;
                    first
                        .is_ascii_uppercase()
                        .then_some((kind.to_string(), name))
                };
                let (string, (kind, name)) = match (quoted(left), variant(right)) {
                    (Some(string), Some(found)) => (string, found),
                    _ => match (variant(left), quoted(right)) {
                        (Some(found), Some(string)) => (string, found),
                        _ => continue,
                    },
                };
                let spoken = norm(&string.replace(['-', ' '], ""));
                let own = norm(&name);
                let agrees = spoken == own || (kind == "PageBoundary" && spoken + "box" == own);
                if !agrees {
                    problems.push(format!(
                        "{}: \"{string}\" is wired to {kind}::{name}",
                        path.display()
                    ));
                }
                checked += 1;
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    assert!(
        checked >= 200,
        "only {checked} string arms found, which means the reader is not reading the bindings"
    );
}
