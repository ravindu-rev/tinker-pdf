//! One version, and the manifests that are supposed to agree with it.
//!
//! Four ecosystems ship this engine and each keeps its version somewhere
//! different: crates.io reads `[package] version`, the wheel reads
//! `bindings/python/Cargo.toml`, the npm package reads
//! `bindings/js/Cargo.toml`, and NuGet reads a `<Version>` element in an XML
//! project file. Nothing makes them agree. A release that bumps the workspace
//! and forgets the `.csproj` ships `TinkerPdf 0.0.1` beside `tinker-pdf
//! 0.0.2`, and nobody finds out until somebody files a bug against a version
//! pair that never existed together.
//!
//! **This checks and does not rewrite.** Plan 26 is explicit about why: a
//! silent correction hides which one was wrong. If the workspace is 0.0.2 and
//! the `.csproj` is 0.0.1, the interesting question is whether the release was
//! meant to be 0.0.2 — and a script that quietly edits the `.csproj` answers
//! it by assumption. So the check names the file, the version it found and the
//! version it wanted, and stops.
//!
//! ## The two halves
//!
//! **Versions.** The workspace's `[workspace.package] version` is the one
//! source. Every crate inside the workspace must inherit it with
//! `version.workspace = true` rather than repeating it, so those cannot drift
//! at all; the check enforces the inheritance rather than the number. The four
//! manifests outside the workspace repeat it literally, because cargo,
//! maturin, wasm-pack and MSBuild have no way to reach a workspace they are
//! not in, and those are the ones compared.
//!
//! `bindings/python/pyproject.toml` is a fifth place a version could live, and
//! the check makes sure it does not: `dynamic = ["version"]` is what makes
//! maturin take the number from `Cargo.toml`, and a literal there would be a
//! place to drift that nothing else in this file could see.
//!
//! **Publishability.** `publish = false` is the only thing standing between
//! `cargo publish` and a crates.io entry for a repository chore. It is checked
//! in *both* directions, which is the half that is easy to leave out: a
//! missing `publish = false` puts `xtask` on crates.io, and a stray one on an
//! engine crate makes the release skip a crate the facade depends on — a
//! failure that surfaces as the *next* crate in the order being unpublishable,
//! which is a long way from the manifest that caused it.

use std::path::Path;

/// Where a package writes its version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A cargo manifest outside the workspace: `[package] version = "x"`.
    Cargo,
    /// An MSBuild project: `<Version>x</Version>`.
    MsBuild,
}

/// The manifests that carry a literal version and must match the workspace.
///
/// Everything under `crates/` is absent on purpose: those inherit with
/// `version.workspace = true` and are checked for the inheritance instead, so
/// there is no number in them to compare.
pub const EXTERNAL: &[(&str, Kind)] = &[
    ("bindings/python/Cargo.toml", Kind::Cargo),
    ("bindings/js/Cargo.toml", Kind::Cargo),
    ("bindings/dotnet/TinkerPdf.csproj", Kind::MsBuild),
];

/// Crates that must never reach crates.io, and the reason each is here.
///
/// The reason is in the table rather than in a comment because it is what the
/// failure message says. "xtask is missing `publish = false`" is a fact;
/// "xtask is repository automation and would be an entry on crates.io nobody
/// could use" is why anybody should care.
pub const NEVER_PUBLISHED: &[(&str, &str)] = &[
    (
        "xtask",
        "repository automation, useless outside this checkout",
    ),
    ("tools/tpdf", "a debug CLI, not a product"),
    ("tools/pdfcmp", "a test comparator, not a product"),
    (
        "tools/oracle-diff",
        "a test harness that shells out to other engines",
    ),
    (
        "fuzz",
        "a libFuzzer harness; it links a C++ runtime the engine refuses",
    ),
    ("bindings/python", "ships as a wheel, through maturin"),
    ("bindings/js", "ships as an npm package, through wasm-pack"),
];

/// Runs both halves and returns every problem, not the first.
pub fn check(root: &Path) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    let workspace = match workspace_version(root) {
        Ok(version) => version,
        Err(problem) => return Err(vec![problem]),
    };

    check_workspace_members(root, &mut problems);
    check_workspace_dependencies(root, &workspace, &mut problems);
    check_external(root, &workspace, &mut problems);
    check_pyproject(root, &mut problems);
    check_publish_flags(root, &mut problems);

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The `[workspace.package] version` of the root manifest.
pub fn workspace_version(root: &Path) -> Result<String, String> {
    let path = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    table_string(&text, "[workspace.package]", "version").ok_or_else(|| {
        "Cargo.toml has no [workspace.package] version, so nothing else has anything to \
         agree with"
            .to_string()
    })
}

/// Every crate in the workspace inherits rather than repeating.
fn check_workspace_members(root: &Path, problems: &mut Vec<String>) {
    for (path, text) in workspace_manifests(root) {
        match package_version(&text) {
            Some(Declared::Inherited) => {}
            Some(Declared::Literal(found)) => problems.push(format!(
                "{path} sets version = \"{found}\" instead of version.workspace = true, so it \
                 can drift from the workspace and nothing but this check would see it"
            )),
            None => problems.push(format!("{path} declares no version at all")),
        }
    }
}

/// Every internal `[workspace.dependencies]` entry names the same version.
///
/// These carry a `version` beside the `path` because crates.io refuses a
/// published crate whose dependency has only a path. Cargo uses the path when
/// building here and the version when resolving from the registry, so the
/// number is invisible until somebody installs the published crate — at which
/// point it selects whatever version it names, which may not be the one that
/// was released alongside it.
fn check_workspace_dependencies(root: &Path, workspace: &str, problems: &mut Vec<String>) {
    let path = root.join("Cargo.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        problems.push("Cargo.toml could not be read".to_string());
        return;
    };
    for (name, version) in internal_dependency_versions(&text) {
        match version {
            Some(found) if found == workspace => {}
            Some(found) => problems.push(format!(
                "Cargo.toml: [workspace.dependencies] {name} says version = \"{found}\", the \
                 workspace is \"{workspace}\" — a published {name} would resolve to a version \
                 this release does not contain"
            )),
            None => problems.push(format!(
                "Cargo.toml: [workspace.dependencies] {name} has a path and no version, so \
                 `cargo publish` refuses every crate that depends on it"
            )),
        }
    }
}

/// The manifests outside the workspace repeat the number, so compare it.
fn check_external(root: &Path, workspace: &str, problems: &mut Vec<String>) {
    for (relative, kind) in EXTERNAL {
        let path = root.join(relative);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            // Not a skip. A version check that passes over a manifest it
            // cannot find is a version check that does not run.
            Err(error) => {
                problems.push(format!(
                    "{relative} could not be read ({error}), so its version went unchecked"
                ));
                continue;
            }
        };
        let found = match kind {
            Kind::Cargo => match package_version(&text) {
                Some(Declared::Literal(found)) => Some(found),
                // Outside the workspace there is no workspace to inherit from;
                // cargo fails the build, but say why rather than letting that
                // be the message.
                Some(Declared::Inherited) => {
                    problems.push(format!(
                        "{relative} says version.workspace = true, but it is outside the \
                         workspace and has nothing to inherit from"
                    ));
                    continue;
                }
                None => None,
            },
            Kind::MsBuild => element(&text, "Version"),
        };
        match found {
            Some(found) if found == workspace => {}
            Some(found) => problems.push(format!(
                "{relative} is version {found}, the workspace is {workspace} — one of the two \
                 is wrong and this check does not guess which"
            )),
            None => problems.push(format!("{relative} declares no version")),
        }
    }
}

/// The wheel's version must keep coming from `Cargo.toml`.
fn check_pyproject(root: &Path, problems: &mut Vec<String>) {
    let relative = "bindings/python/pyproject.toml";
    let text = match std::fs::read_to_string(root.join(relative)) {
        Ok(text) => text,
        Err(error) => {
            problems.push(format!("{relative} could not be read ({error})"));
            return;
        }
    };
    if !text.contains("dynamic = [\"version\"]") {
        problems.push(format!(
            "{relative} does not declare dynamic = [\"version\"], so the wheel's version comes \
             from somewhere other than Cargo.toml and is a fifth place to drift"
        ));
    }
}

/// `publish = false` where it belongs, and nowhere else.
fn check_publish_flags(root: &Path, problems: &mut Vec<String>) {
    for (relative, reason) in NEVER_PUBLISHED {
        let path = root.join(relative).join("Cargo.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                problems.push(format!(
                    "{relative}/Cargo.toml could not be read ({error}), so nothing checked \
                     whether it can reach crates.io"
                ));
                continue;
            }
        };
        if !is_publish_false(&text) {
            problems.push(format!(
                "{relative} has no `publish = false`, so `cargo publish` there would put it on \
                 crates.io — {reason}"
            ));
        }
    }

    for (path, text) in workspace_manifests(root) {
        if !path.starts_with("crates/") {
            continue;
        }
        if is_publish_false(&text) {
            problems.push(format!(
                "{path} says `publish = false`, but every crate under crates/ is part of the \
                 released set — the release would skip it and the next crate in dependency \
                 order would fail with an error naming a different crate"
            ));
        }
    }
}

/// `<crate path, manifest text>` for every member of the workspace.
///
/// Read from the root manifest's `members` list rather than by walking
/// `crates/`, so a member added anywhere is covered and a stray directory is
/// not.
pub fn workspace_manifests(root: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(root.join("Cargo.toml")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for member in list_strings(&text, "[workspace]", "members") {
        if let Ok(text) = std::fs::read_to_string(root.join(&member).join("Cargo.toml")) {
            out.push((member, text));
        }
    }
    out
}

/// How a manifest states its version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Declared {
    /// `version.workspace = true`.
    Inherited,
    /// `version = "x"`.
    Literal(String),
}

/// The `[package]` table's version, in whichever of the two forms it takes.
pub fn package_version(manifest: &str) -> Option<Declared> {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("version.workspace") {
            if rest.trim_start().starts_with('=') {
                return Some(Declared::Inherited);
            }
        }
        if let Some(rest) = line.strip_prefix("version") {
            let rest = rest.trim_start();
            let Some(rest) = rest.strip_prefix('=') else {
                continue;
            };
            let rest = rest.trim();
            if let Some(value) = quoted(rest) {
                return Some(Declared::Literal(value));
            }
            // `version = { workspace = true }` is the long spelling of the
            // same thing.
            if rest.contains("workspace") && rest.contains("true") {
                return Some(Declared::Inherited);
            }
        }
    }
    None
}

/// `publish = false` anywhere in the `[package]` table.
pub fn is_publish_false(manifest: &str) -> bool {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("publish") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                return rest.trim().starts_with("false");
            }
        }
    }
    false
}

/// `[workspace.dependencies]`'s `tinker-pdf*` entries and the version each
/// names, if any.
pub fn internal_dependency_versions(manifest: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut in_table = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_table = line == "[workspace.dependencies]";
            continue;
        }
        if !in_table || line.starts_with('#') || line.is_empty() {
            continue;
        }
        let Some((name, rest)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if !name.starts_with("tinker-pdf") {
            continue;
        }
        out.push((name.to_string(), inline_key(rest, "version")));
    }
    out
}

/// A `key = "value"` from an inline table, ignoring any other key.
fn inline_key(inline: &str, key: &str) -> Option<String> {
    let mut rest = inline;
    while let Some(at) = rest.find(key) {
        let after = &rest[at + key.len()..];
        // `version` must not match the `version` inside `default-features`
        // or a path segment; require the next non-space character to be `=`.
        let trimmed = after.trim_start();
        let before_is_boundary = rest[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '-' && c != '_');
        if before_is_boundary {
            if let Some(value) = trimmed.strip_prefix('=') {
                if let Some(found) = quoted(value.trim()) {
                    return Some(found);
                }
            }
        }
        rest = after;
    }
    None
}

/// The text of the first `<name>…</name>` element.
pub fn element(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].trim().to_string())
}

/// A `key = "value"` inside a named table.
fn table_string(text: &str, table: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == table;
            continue;
        }
        if !inside || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                return quoted(rest.trim());
            }
        }
    }
    None
}

/// A `key = [ "a", "b" ]` inside a named table, however it is wrapped.
fn list_strings(text: &str, table: &str, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    let mut collecting = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if !collecting && trimmed.starts_with('[') && !trimmed.starts_with(key) {
            inside = trimmed == table;
            continue;
        }
        if !inside {
            continue;
        }
        if !collecting {
            let Some(rest) = trimmed.strip_prefix(key) else {
                continue;
            };
            let rest = rest.trim_start();
            let Some(rest) = rest.strip_prefix('=') else {
                continue;
            };
            collecting = true;
            push_quoted(rest, &mut out);
            if rest.contains(']') {
                break;
            }
            continue;
        }
        push_quoted(trimmed, &mut out);
        if trimmed.contains(']') {
            break;
        }
    }
    out
}

fn push_quoted(line: &str, out: &mut Vec<String>) {
    let mut rest = line;
    while let Some(value) = quoted(rest) {
        let consumed = rest.find('"').unwrap_or(0) + 1 + value.len() + 1;
        out.push(value);
        rest = &rest[consumed.min(rest.len())..];
    }
}

/// The contents of the first double-quoted run.
fn quoted(text: &str) -> Option<String> {
    let start = text.find('"')? + 1;
    let end = text[start..].find('"')? + start;
    Some(text[start..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo_root;

    #[test]
    fn an_inherited_version_is_recognised_in_both_spellings() {
        assert_eq!(
            package_version("[package]\nversion.workspace = true\n"),
            Some(Declared::Inherited)
        );
        assert_eq!(
            package_version("[package]\nversion = { workspace = true }\n"),
            Some(Declared::Inherited)
        );
    }

    #[test]
    fn a_literal_version_is_read() {
        assert_eq!(
            package_version("[package]\nname = \"x\"\nversion = \"0.0.1\"\n"),
            Some(Declared::Literal("0.0.1".to_string()))
        );
    }

    /// A version in some other table is not the package's. `[workspace.package]`
    /// carries one in the root manifest and a member manifest may carry a
    /// `[dependencies]` entry with a version on its own line.
    #[test]
    fn a_version_outside_the_package_table_is_not_the_packages() {
        let manifest = "[workspace.package]\nversion = \"9.9.9\"\n\n\
                        [package]\nversion = \"0.0.1\"\n";
        assert_eq!(
            package_version(manifest),
            Some(Declared::Literal("0.0.1".to_string()))
        );
    }

    #[test]
    fn publish_false_is_read_only_from_the_package_table() {
        assert!(is_publish_false("[package]\npublish = false\n"));
        assert!(!is_publish_false("[package]\npublish = true\n"));
        assert!(!is_publish_false("[package]\nname = \"x\"\n"));
        // A commented-out flag is not a flag, which is the shape a "temporarily
        // let this publish" edit takes.
        assert!(!is_publish_false("[package]\n# publish = false\n"));
    }

    #[test]
    fn an_msbuild_version_element_is_read() {
        let xml = "<Project>\n  <PropertyGroup>\n    <Version>0.0.1</Version>\n\
                   </PropertyGroup>\n</Project>\n";
        assert_eq!(element(xml, "Version").as_deref(), Some("0.0.1"));
    }

    #[test]
    fn a_dependency_version_is_read_past_the_path() {
        let manifest = "[workspace.dependencies]\n\
                        tinker-pdf-cos = { path = \"crates/tinker-pdf-cos\", version = \"0.0.1\", \
                        default-features = false }\n\
                        tinker-pdf-raster = { path = \"crates/tinker-pdf-raster\" }\n";
        assert_eq!(
            internal_dependency_versions(manifest),
            vec![
                ("tinker-pdf-cos".to_string(), Some("0.0.1".to_string())),
                ("tinker-pdf-raster".to_string(), None),
            ]
        );
    }

    #[test]
    fn the_members_list_is_read_across_lines() {
        let manifest =
            "[workspace]\nresolver = \"2\"\nmembers = [\n  \"crates/a\",\n  \"xtask\",\n]\n";
        assert_eq!(
            list_strings(manifest, "[workspace]", "members"),
            vec!["crates/a".to_string(), "xtask".to_string()]
        );
    }

    /// The check runs against this repository, which is the point of it.
    #[test]
    fn this_repository_agrees_with_itself_about_its_version() {
        if let Err(problems) = check(&repo_root()) {
            panic!("the manifests disagree about the version:\n{problems:#?}");
        }
    }

    /// Every member is found. A members list that stopped parsing after the
    /// first entry would make `check_workspace_members` pass vacuously.
    #[test]
    fn every_workspace_member_is_read() {
        let manifests = workspace_manifests(&repo_root());
        assert!(
            manifests.len() >= 15,
            "only {} members found: {:?}",
            manifests.len(),
            manifests.iter().map(|(p, _)| p).collect::<Vec<_>>()
        );
        assert!(manifests.iter().any(|(path, _)| path == "xtask"));
    }
}
