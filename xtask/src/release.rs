//! One tag, four registries, and a dry run that is the default.
//!
//! Plan 13 specifies the shape — `cargo publish`, then maturin, then
//! wasm-pack, then `dotnet pack`, driven from a tag — and plan 26 specifies
//! the safety property: every step has a dry-run mode and the first exercise
//! of the whole pipeline is a dry run end to end, because **a half-published
//! release cannot be retracted from crates.io**.
//!
//! So the polarity is inverted from the usual. `cargo xtask release` is a dry
//! run; `--execute` is the flag that publishes. A tired operator who types the
//! command without its arguments gets a report, not a release, and there is no
//! spelling of "dry run" that can be forgotten.
//!
//! ## Why the order is not a list somebody maintains
//!
//! crates.io refuses a crate whose path dependency is unpublished, so the ten
//! Rust crates must go bottom-up. That order is not written down here: it is
//! computed from the manifests by a topological sort, and then *checked*
//! against the same manifests by [`validate_order`]. Both halves exist on
//! purpose. A hand-maintained list goes stale the first time somebody adds an
//! edge — and it fails in the least helpful possible way, halfway through a
//! publish that cannot be undone, with an error naming the crate that came
//! *after* the mistake. Computing it removes the staleness; validating it
//! means a future hand-edit, a sort with a subtle bug, or a `--only` filter
//! that drops something is caught before the first upload rather than after
//! the fifth.
//!
//! ## Why the language packages build against the published facade
//!
//! Plan 26 records the choice and the reason: "building against the workspace
//! keeps a release atomic, building against the published crate proves the
//! published crate works", and it takes the second, for the same reason the
//! tools depend on the facade rather than reaching past it. So the wheel, the
//! npm package and the NuGet package are built after the crates.io stage, from
//! a manifest whose `tinker-pdf` dependency has been switched from a path to a
//! version.
//!
//! In dry-run mode that switch cannot be made — there is nothing on crates.io
//! to switch to — so the pipeline says what it would do instead of pretending
//! it did it. [`Step::registry_note`] carries that text, and every dry run
//! prints it.
//!
//! ## Resumability without a network call
//!
//! Plan 13 asks that re-running a release skip what is already published.
//! There is no registry query here, deliberately: an "is it published?" probe
//! is a second source of truth that can disagree with the first, and it makes
//! the release fail when the network hiccups. Instead the executing path reads
//! the error cargo already gives — crates.io answers a duplicate upload with
//! "already uploaded" — and treats *that one* failure as a completed step.
//! Every other failure halts the chain, which is what plan 13 asks for.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::version;

/// A stage of the release, in the order plan 13 gives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The preflight checks. Never publishes anything.
    Preflight,
    /// The Rust crates, bottom-up.
    Crates,
    /// The Python wheel, through maturin.
    Wheel,
    /// The npm package, through wasm-pack.
    Npm,
    /// The NuGet package, through the .NET SDK.
    NuGet,
}

impl Stage {
    /// The name `--only` takes.
    pub fn name(self) -> &'static str {
        match self {
            Stage::Preflight => "preflight",
            Stage::Crates => "crates",
            Stage::Wheel => "wheel",
            Stage::Npm => "npm",
            Stage::NuGet => "nuget",
        }
    }

    fn parse(name: &str) -> Option<Stage> {
        [
            Stage::Preflight,
            Stage::Crates,
            Stage::Wheel,
            Stage::Npm,
            Stage::NuGet,
        ]
        .into_iter()
        .find(|stage| stage.name() == name)
    }
}

/// One command the release would run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// Which stage it belongs to.
    pub stage: Stage,
    /// What it is for, in one line.
    pub label: String,
    /// The program and its arguments, as a real run would invoke them.
    pub live: Vec<String>,
    /// The same step with the tool's own dry-run flags, if it has any.
    ///
    /// `None` means the step publishes and has no harmless form, so a dry run
    /// prints it and does not run it. That is the case for every actual upload:
    /// `cargo publish --dry-run` exists, `npm publish --dry-run` exists,
    /// `dotnet nuget push` has none worth trusting, and `maturin publish` has
    /// none at all — for those the dry run substitutes the *build*, which is
    /// the half that can fail for a reason worth knowing about in advance.
    pub dry: Option<Vec<String>>,
    /// Whether the live command uploads to a registry.
    ///
    /// The field exists so a test can state the safety property rather than
    /// infer it: **no step with this set may run its live command in a dry
    /// run.** Preflight and the three build steps are read-only in both
    /// modes, so "the dry command differs from the live one" is not the rule
    /// and asserting it would be asserting the wrong thing.
    pub publishes: bool,
    /// Where to run it, relative to the repository root.
    pub cwd: String,
    /// What a dry run cannot prove about this step, if anything.
    pub registry_note: Option<String>,
}

impl Step {
    /// The command a real run would issue, as one line.
    pub fn live_line(&self) -> String {
        self.live.join(" ")
    }

    /// The command this dry run issues, if it issues one.
    pub fn dry_line(&self) -> Option<String> {
        self.dry.as_ref().map(|args| args.join(" "))
    }
}

/// What the command line said.
///
/// `Default` is the dry run, and that is load-bearing rather than incidental:
/// every path that builds an `Options` without one — the no-argument command
/// line, every test — gets the harmless behaviour, so there is no way to add a
/// caller that publishes by omission.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Publish for real. Absent, the run is a dry run.
    pub execute: bool,
    /// Run only these stages. Empty means all of them.
    pub only: Vec<Stage>,
    /// The tag being released, if one was named.
    pub tag: Option<String>,
    /// Print the plan and run nothing at all, not even the dry-run commands.
    pub plan_only: bool,
}

/// Parses `release`'s arguments.
pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            // Accepted and ignored: it is the default, and refusing to spell
            // out what you are doing is a bad trade for a release command.
            "--dry-run" => options.execute = false,
            "--execute" => options.execute = true,
            "--plan" => options.plan_only = true,
            "--only" => {
                let name = rest
                    .next()
                    .ok_or_else(|| "--only needs a stage name".to_string())?;
                let stage =
                    Stage::parse(name).ok_or_else(|| format!("--only {name}: no such stage"))?;
                options.only.push(stage);
            }
            "--tag" => {
                let tag = rest.next().ok_or_else(|| "--tag needs a tag".to_string())?;
                options.tag = Some(tag.clone());
            }
            other => return Err(format!("release: unknown argument `{other}`")),
        }
    }
    Ok(options)
}

/// The crates that go to crates.io, in an order that publishes every crate
/// after everything it depends on.
///
/// Kahn's algorithm over the internal dependency graph, with ties broken by
/// name so the order is the same on every machine and in every log.
pub fn publish_order(graph: &[(String, Vec<String>)]) -> Result<Vec<String>, String> {
    let names: Vec<&str> = graph.iter().map(|(name, _)| name.as_str()).collect();
    let mut remaining: Vec<(String, Vec<String>)> = graph
        .iter()
        .map(|(name, deps)| {
            let deps = deps
                .iter()
                .filter(|dep| names.contains(&dep.as_str()))
                .cloned()
                .collect();
            (name.clone(), deps)
        })
        .collect();
    remaining.sort_by(|a, b| a.0.cmp(&b.0));

    let mut order: Vec<String> = Vec::new();
    while !remaining.is_empty() {
        let ready: Vec<String> = remaining
            .iter()
            .filter(|(_, deps)| deps.iter().all(|dep| order.contains(dep)))
            .map(|(name, _)| name.clone())
            .collect();
        if ready.is_empty() {
            let stuck: Vec<&str> = remaining.iter().map(|(n, _)| n.as_str()).collect();
            return Err(format!(
                "the crate graph has a cycle through {}; crates.io cannot publish any of them",
                stuck.join(", ")
            ));
        }
        remaining.retain(|(name, _)| !ready.contains(name));
        order.extend(ready);
    }
    Ok(order)
}

/// Every crate in `order` comes after everything it depends on.
///
/// Separate from [`publish_order`] on purpose. The sort is one implementation
/// of an ordering rule; this is the rule. A sort that is subtly wrong, an
/// order somebody pasted in by hand, and a filter that dropped a crate are all
/// caught here, and this is the function the release calls before it uploads
/// anything.
pub fn validate_order(
    order: &[String],
    graph: &[(String, Vec<String>)],
) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for name in order {
        let Some((_, deps)) = graph.iter().find(|(n, _)| n == name) else {
            problems.push(format!(
                "{name} is in the publish order and is not a crate in this workspace"
            ));
            continue;
        };
        for dep in deps {
            // A dependency outside the released set is somebody else's
            // problem: it is on crates.io already or it is not published at
            // all, and either way this order does not decide it.
            if !graph.iter().any(|(n, _)| n == dep) {
                continue;
            }
            if !seen.contains(&dep.as_str()) {
                problems.push(format!(
                    "{name} is published before {dep}, which it depends on — crates.io rejects a \
                     crate whose dependency is not there yet, so the release would halt at \
                     {name} having already uploaded {} crate(s) that cannot be taken back",
                    seen.len()
                ));
            }
        }
        seen.push(name);
    }
    for (name, _) in graph {
        if !order.contains(name) {
            problems.push(format!(
                "{name} is in the workspace and not in the publish order, so nothing would \
                 publish it and every crate depending on it would fail"
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The dependency graph of the crates that publish, read from the manifests.
///
/// `crates/` only. The tools, `xtask` and the two binding crates carry
/// `publish = false` and are not in the released set at all; `cargo xtask
/// versions` is what keeps that true.
pub fn released_graph(root: &Path) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for (path, text) in version::workspace_manifests(root) {
        if !path.starts_with("crates/") || version::is_publish_false(&text) {
            continue;
        }
        if let Some(name) = version::package_name(&text) {
            out.push((name, version::internal_dependencies(&text)));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Every file a crate would upload, and how many bytes of them there are.
///
/// `cargo package --list` is the only part of packaging that works without the
/// registry, which makes it the only part a dry run of a multi-crate release
/// can exercise for the crates above the leaves. It is worth exercising: the
/// file *set* is where a release goes wrong quietly.
pub fn package_contents(root: &Path, crate_name: &str) -> Result<(Vec<String>, u64), String> {
    let output = Command::new("cargo")
        .args(["package", "--list", "--allow-dirty", "-p", crate_name])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cargo could not be started ({e})"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo package --list -p {crate_name} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let listed: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().replace('\\', "/"))
        .filter(|line| !line.is_empty())
        .collect();
    // Cargo synthesises three of these, so they are not on disk to measure.
    let mut bytes = 0u64;
    let directory = root.join("crates").join(crate_name);
    for entry in &listed {
        if let Ok(meta) = std::fs::metadata(directory.join(entry)) {
            bytes += meta.len();
        }
    }
    Ok((listed, bytes))
}

/// crates.io refuses an upload above this, and says so only at the upload.
pub const CRATES_IO_CEILING: u64 = 10 * 1024 * 1024;

/// Vendored data a build script reads must be inside the package.
///
/// This is a real defect this pipeline found rather than a hypothetical.
/// `tinker-pdf-font` carried `exclude = ["data/"]`, reasoning that Adobe's
/// CMap registry is compiled by `build.rs` and not read at run time — true,
/// and beside the point, because build time is *the consumer's* build time.
/// The excluded crate uploads perfectly and then fails to build for everybody
/// who downloads it, with `no CMaps under ... the vendored data is missing`,
/// and every crate above it in the graph inherits the failure. Four crates
/// would already have been on crates.io by then.
///
/// The rule is narrow and mechanical: a crate with a non-empty `data/`
/// directory must ship files from it. That directory is exactly where this
/// repository puts things a build script reads — `cargo xtask vendor` uses the
/// same convention — so the two checks cover the same tree from two sides.
pub fn vendored_data_reaches_the_package(root: &Path) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    for (name, _) in released_graph(root) {
        let data = root.join("crates").join(&name).join("data");
        if !data.is_dir() || std::fs::read_dir(&data).into_iter().flatten().count() == 0 {
            continue;
        }
        match package_contents(root, &name) {
            Ok((listed, _)) => {
                if !listed.iter().any(|path| path.starts_with("data/")) {
                    problems.push(format!(
                        "{name} vendors data/ and would publish none of it. Its build script \
                         reads that directory, so the uploaded crate builds here and nowhere \
                         else — check `exclude` in crates/{name}/Cargo.toml"
                    ));
                }
            }
            Err(why) => problems.push(format!("{name}: {why}")),
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// This machine's .NET runtime identifier, and what a cdylib is called on it.
///
/// A RID is how NuGet carries platform binaries: a package holds
/// `runtimes/<rid>/native/<library>` for every platform it supports, and the
/// .NET host picks one at run time. Nothing in cargo knows the word "RID", so
/// the mapping lives here.
pub fn host_rid() -> Result<(&'static str, String), String> {
    let rid = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "win-x64",
        ("windows", "aarch64") => "win-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("macos", "x86_64") => "osx-x64",
        ("macos", "aarch64") => "osx-arm64",
        (os, arch) => {
            return Err(format!(
                "no .NET runtime identifier is mapped for {os}/{arch}; add one rather than \
                 guessing, because a wrong RID packs a library the host will never load"
            ))
        }
    };
    let library = format!(
        "{}tinker_pdf_ffi{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    Ok((rid, library))
}

/// Where a RID's native library belongs, relative to the repository root.
pub fn native_path(rid: &str, library: &str) -> String {
    format!("bindings/dotnet/runtimes/{rid}/native/{library}")
}

/// Copies this machine's freshly built cdylib into the per-RID layout.
///
/// The layout is the whole of what makes a NuGet package carry a native
/// library, and until this existed `dotnet pack` produced a perfectly valid
/// managed-only package: `TinkerPdf.dll` resolves, every `DllImport` in it
/// names `tinker_pdf_ffi`, and the first call throws `DllNotFoundException` at
/// run time on the consumer's machine. That is why
/// `bindings/dotnet/tests/Smoke` consumes the *package* rather than the
/// project — a `ProjectReference` finds the cdylib wherever cargo left it and
/// passes on a package that has none.
pub fn stage_native_library(root: &Path) -> Result<String, String> {
    let (rid, library) = host_rid()?;
    let built = root.join("target/release").join(&library);
    if !built.is_file() {
        return Err(format!(
            "{} does not exist; build it with `cargo build --release -p tinker-pdf-ffi` first",
            built.display()
        ));
    }
    let destination = root.join(native_path(rid, &library));
    let directory = destination
        .parent()
        .ok_or_else(|| "the native path has no directory".to_string())?;
    std::fs::create_dir_all(directory).map_err(|e| format!("{}: {e}", directory.display()))?;
    std::fs::copy(&built, &destination).map_err(|e| format!("{}: {e}", destination.display()))?;
    let bytes = std::fs::metadata(&destination)
        .map(|m| m.len())
        .unwrap_or(0);
    Ok(format!(
        "{} ({bytes} bytes) — this machine's RID only; the others come from the release \
         workflow's matrix",
        native_path(rid, &library)
    ))
}

/// Every RID directory that currently holds a native library.
pub fn staged_rids(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root.join("bindings/dotnet/runtimes")) else {
        return out;
    };
    for entry in entries.flatten() {
        let native = entry.path().join("native");
        let populated = std::fs::read_dir(&native)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false);
        if populated {
            out.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    out.sort();
    out
}

/// The whole pipeline, as the list of commands it is.
pub fn plan(root: &Path, options: &Options) -> Result<Vec<Step>, String> {
    let workspace = version::workspace_version(root)?;
    let graph = released_graph(root);
    let order = publish_order(&graph)?;
    if let Err(problems) = validate_order(&order, &graph) {
        return Err(problems.join("\n"));
    }

    let mut steps = Vec::new();

    steps.push(Step {
        stage: Stage::Preflight,
        label: "the manifests agree with the workspace version, and the graph is legal".into(),
        live: vec!["cargo".into(), "xtask".into(), "check".into()],
        dry: Some(vec!["cargo".into(), "xtask".into(), "check".into()]),
        publishes: false,
        cwd: ".".into(),
        registry_note: None,
    });
    steps.push(Step {
        stage: Stage::Preflight,
        label: "the workspace builds and its tests pass on this platform".into(),
        live: vec!["cargo".into(), "test".into(), "--workspace".into()],
        dry: Some(vec!["cargo".into(), "test".into(), "--workspace".into()]),
        publishes: false,
        cwd: ".".into(),
        registry_note: None,
    });

    for name in &order {
        steps.push(Step {
            stage: Stage::Crates,
            label: format!("{name} to crates.io"),
            live: vec![
                "cargo".into(),
                "publish".into(),
                "-p".into(),
                name.clone(),
                "--locked".into(),
            ],
            dry: Some(vec![
                "cargo".into(),
                "publish".into(),
                "-p".into(),
                name.clone(),
                "--locked".into(),
                "--dry-run".into(),
            ]),
            publishes: true,
            cwd: ".".into(),
            registry_note: None,
        });
    }

    let published_facade = format!(
        "a real run would first rewrite this manifest's `tinker-pdf` dependency from a path to \
         version {workspace}, so the package is built against the crate that was just \
         published rather than against this checkout (plan 26). A dry run cannot: there is no \
         {workspace} on crates.io to build against, so what is exercised here is the same code \
         reached through the path"
    );

    steps.push(Step {
        stage: Stage::Wheel,
        label: "an abi3 wheel for this platform".into(),
        live: vec![
            "maturin".into(),
            "publish".into(),
            "--manifest-path".into(),
            "bindings/python/Cargo.toml".into(),
            "--release".into(),
            "--skip-existing".into(),
        ],
        // maturin has no `--dry-run`, so the dry form is the build. It is the
        // half that can fail for a reason worth knowing in advance; the upload
        // is a POST.
        dry: Some(vec![
            "maturin".into(),
            "build".into(),
            "--manifest-path".into(),
            "bindings/python/Cargo.toml".into(),
            "--release".into(),
            "--out".into(),
            "target/release-dry-run/wheels".into(),
        ]),
        publishes: true,
        cwd: ".".into(),
        registry_note: Some(format!(
            "{published_facade}. The wheel built here is for this machine's platform only; the \
             manylinux, musllinux and macOS wheels come from the release workflow's matrix"
        )),
    });

    steps.push(Step {
        stage: Stage::Npm,
        label: "the wasm package".into(),
        live: vec![
            "wasm-pack".into(),
            "build".into(),
            "--release".into(),
            "--target".into(),
            "web".into(),
            "--out-dir".into(),
            "pkg".into(),
            "bindings/js".into(),
        ],
        dry: Some(vec![
            "wasm-pack".into(),
            "build".into(),
            "--release".into(),
            "--target".into(),
            "web".into(),
            "--out-dir".into(),
            "pkg".into(),
            "bindings/js".into(),
        ]),
        publishes: false,
        cwd: ".".into(),
        registry_note: None,
    });
    steps.push(Step {
        stage: Stage::Npm,
        label: "the wasm package to npm".into(),
        live: vec![
            "npm".into(),
            "publish".into(),
            "--access".into(),
            "public".into(),
        ],
        dry: Some(vec![
            "npm".into(),
            "publish".into(),
            "--dry-run".into(),
            "--access".into(),
            "public".into(),
        ]),
        publishes: true,
        cwd: "bindings/js/pkg".into(),
        registry_note: Some(published_facade.clone()),
    });

    steps.push(Step {
        stage: Stage::NuGet,
        label: "the C ABI cdylib for this machine's RID".into(),
        live: vec![
            "cargo".into(),
            "build".into(),
            "--release".into(),
            "-p".into(),
            "tinker-pdf-ffi".into(),
        ],
        dry: Some(vec![
            "cargo".into(),
            "build".into(),
            "--release".into(),
            "-p".into(),
            "tinker-pdf-ffi".into(),
        ]),
        publishes: false,
        cwd: ".".into(),
        registry_note: None,
    });
    steps.push(Step {
        stage: Stage::NuGet,
        label: "the cdylib into bindings/dotnet/runtimes/<rid>/native/".into(),
        live: vec!["cargo".into(), "xtask".into(), "nuget-stage".into()],
        dry: Some(vec!["cargo".into(), "xtask".into(), "nuget-stage".into()]),
        publishes: false,
        cwd: ".".into(),
        registry_note: Some(
            "a real run gathers one cdylib per RID from the release workflow's matrix into \
             this layout before packing. A single machine can build only its own, so a package \
             packed here carries one RID and is not the package that ships"
                .into(),
        ),
    });
    steps.push(Step {
        stage: Stage::NuGet,
        label: "the NuGet package, with every RID's native library".into(),
        live: vec![
            "dotnet".into(),
            "pack".into(),
            "bindings/dotnet/TinkerPdf.csproj".into(),
            "-c".into(),
            "Release".into(),
            "-o".into(),
            "target/nuget".into(),
        ],
        dry: Some(vec![
            "dotnet".into(),
            "pack".into(),
            "bindings/dotnet/TinkerPdf.csproj".into(),
            "-c".into(),
            "Release".into(),
            "-o".into(),
            "target/nuget".into(),
        ]),
        publishes: false,
        cwd: ".".into(),
        registry_note: None,
    });
    steps.push(Step {
        stage: Stage::NuGet,
        label: "the NuGet package to nuget.org".into(),
        live: vec![
            "dotnet".into(),
            "nuget".into(),
            "push".into(),
            format!("target/nuget/TinkerPdf.{workspace}.nupkg"),
            "--source".into(),
            "https://api.nuget.org/v3/index.json".into(),
            "--skip-duplicate".into(),
        ],
        // `dotnet nuget push` has no dry-run. There is nothing harmless to
        // substitute, so a dry run prints it and stops.
        publishes: true,
        dry: None,
        cwd: ".".into(),
        registry_note: Some(published_facade),
    });

    if !options.only.is_empty() {
        steps.retain(|step| options.only.contains(&step.stage));
    }
    Ok(steps)
}

/// Runs the pipeline, or reports what it would run.
pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let options = parse(args)?;
    let workspace = version::workspace_version(root)?;
    let steps = plan(root, &options)?;

    if options.execute {
        // The one place that can publish. Everything above this point is
        // arithmetic over manifests.
        println!("release: EXECUTING — this publishes to crates.io, PyPI, npm and NuGet.");
    } else {
        println!("release: DRY RUN — nothing is published. Pass --execute to publish.");
    }
    println!("release: workspace version {workspace}");

    if let Some(tag) = &options.tag {
        let expected = format!("v{workspace}");
        if *tag != expected {
            return Err(format!(
                "tag {tag} does not match the workspace version, which would be {expected}. \
                 One of the two is wrong and this does not guess which"
            ));
        }
        println!("release: tag {tag} matches");
    } else {
        println!("release: no --tag given, so nothing checked that the tag and the version agree");
    }

    let released: Vec<String> = released_graph(root)
        .into_iter()
        .map(|(name, _)| name)
        .collect();

    // Before anything is uploaded, and printed even in a dry run, because the
    // file set is the half of packaging that a dry run of a multi-crate
    // release can still check for every crate rather than only for the leaves.
    if !options.plan_only && steps.iter().any(|step| step.stage == Stage::Crates) {
        println!("\nrelease: what each crate would upload");
        for name in &released {
            match package_contents(root, name) {
                Ok((listed, bytes)) => println!(
                    "release:   {name}: {} files, {} KiB before compression (crates.io refuses \
                     above {} MiB compressed)",
                    listed.len(),
                    bytes / 1024,
                    CRATES_IO_CEILING / 1024 / 1024
                ),
                Err(why) => return Err(why),
            }
        }
        if let Err(problems) = vendored_data_reaches_the_package(root) {
            return Err(problems.join("\n"));
        }
    }

    let mut ran = 0usize;
    let mut skipped = 0usize;
    let mut unprovable = 0usize;
    for (index, step) in steps.iter().enumerate() {
        let number = index + 1;
        println!(
            "\nrelease: step {number}/{} [{}]",
            steps.len(),
            step.stage.name()
        );
        println!("release:   {}", step.label);
        println!("release:   would run: {}", step.live_line());
        if step.cwd != "." {
            println!("release:   in: {}", step.cwd);
        }
        if let Some(note) = &step.registry_note {
            println!("release:   note: {note}");
        }

        let command = if options.execute {
            Some(step.live.clone())
        } else {
            step.dry.clone()
        };
        let Some(command) = command else {
            println!(
                "release:   SKIPPED in a dry run: this step publishes and the tool has no \
                 harmless form"
            );
            skipped += 1;
            continue;
        };
        if options.plan_only {
            println!("release:   --plan: not run");
            skipped += 1;
            continue;
        }
        println!("release:   running: {}", command.join(" "));
        match invoke(root, &step.cwd, &command, options.execute, &released) {
            Outcome::Ok => {
                println!("release:   ok");
                ran += 1;
            }
            Outcome::AlreadyPublished => {
                // Plan 13's resumability, without a registry probe.
                println!("release:   already published at this version; continuing");
                ran += 1;
            }
            Outcome::NotYetPublished(name) => {
                println!(
                    "release:   UNPROVABLE in a dry run: this crate depends on {name}, which \
                     an earlier step of this very release would have published. `cargo publish \
                     --dry-run` resolves against the live index, so it cannot see a crate the \
                     dry run declined to upload. A real run reaches this step with {name} \
                     already on crates.io. Continuing."
                );
                unprovable += 1;
            }
            Outcome::Failed(why) => {
                return Err(format!(
                    "step {number} ({}) failed: {why}\n\
                     The chain halts here, which is the point: {} step(s) had already run.",
                    step.label, ran
                ));
            }
        }
    }

    println!(
        "\nrelease: {} step(s) ran, {} skipped, {} unprovable without publishing, {} planned",
        ran,
        skipped,
        unprovable,
        steps.len()
    );
    if !options.execute {
        println!("release: nothing was published.");
    }
    Ok(())
}

/// How one invocation went.
enum Outcome {
    Ok,
    /// crates.io, PyPI, npm and NuGet all answer a duplicate with a message
    /// rather than a distinct status, so this is read out of the output.
    AlreadyPublished,
    /// A dry run asked crates.io for a crate this same release would have
    /// published a few steps earlier, and it is not there because the dry run
    /// did not upload it. Named so the report can say which.
    NotYetPublished(String),
    Failed(String),
}

fn invoke(
    root: &Path,
    cwd: &str,
    command: &[String],
    execute: bool,
    released: &[String],
) -> Outcome {
    let Some((program, args)) = command.split_first() else {
        return Outcome::Failed("empty command".into());
    };
    let directory: PathBuf = root.join(cwd);
    let output = Command::new(program)
        .args(args)
        .current_dir(&directory)
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            return Outcome::Failed(format!(
                "{program} could not be started ({error}). It is build tooling, not a \
                 dependency, so it is installed separately"
            ))
        }
    };
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    for line in text.lines() {
        println!("release:     | {line}");
    }
    if output.status.success() {
        return Outcome::Ok;
    }
    if is_already_published(&text) {
        return Outcome::AlreadyPublished;
    }
    // Only in a dry run, and only for a crate this release publishes itself.
    // A real run that cannot find a dependency has a real problem, and a
    // missing *third-party* crate is a real problem in either mode.
    if !execute {
        if let Some(name) = missing_internal_dependency(&text, released) {
            return Outcome::NotYetPublished(name);
        }
    }
    Outcome::Failed(format!("{program} exited with {}", output.status))
}

/// The crate cargo could not find, if it is one this release publishes.
///
/// This is the single most confusing thing about a dry run of a multi-crate
/// release and it is worth naming precisely rather than swallowing. `cargo
/// publish --dry-run` still resolves dependencies against the real index, so
/// every crate above the leaves fails with `no matching package named X` —
/// which reads exactly like a broken manifest and is not one.
///
/// Narrow on purpose. `X` must be a crate in this workspace's released set; a
/// genuine typo in a third-party dependency name produces the same sentence
/// and must still halt the chain.
pub fn missing_internal_dependency(output: &str, released: &[String]) -> Option<String> {
    for line in output.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("no matching package named `") else {
            continue;
        };
        let Some((name, _)) = rest.split_once('`') else {
            continue;
        };
        if released.iter().any(|crate_name| crate_name == name) {
            return Some(name.to_string());
        }
    }
    None
}

/// Whether a registry refused because this version is already there.
///
/// Matched on text because none of the four tools reports it as a status code
/// of its own. Deliberately narrow: anything not matched here halts the chain,
/// which is the safe direction — a missed match costs a manual re-run, and a
/// loose match would let a real failure be read as "already done" and let the
/// release carry on past it.
pub fn is_already_published(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    [
        "already uploaded",
        "already exists",
        "crate version is already",
        "file name conflict",
        "cannot publish over the previously published version",
        "conflict: package already exists",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo_root;

    fn graph(pairs: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        pairs
            .iter()
            .map(|(name, deps)| {
                (
                    (*name).to_string(),
                    deps.iter().map(|d| (*d).to_string()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn a_dependency_is_published_before_what_depends_on_it() {
        let g = graph(&[("facade", &["leaf"]), ("leaf", &[]), ("mid", &["leaf"])]);
        let order = publish_order(&g).expect("acyclic");
        assert_eq!(order.first().map(String::as_str), Some("leaf"));
        validate_order(&order, &g).expect("the computed order is legal");
    }

    /// The order is the same every time, so a log from one machine can be
    /// compared with a log from another.
    #[test]
    fn the_order_is_stable() {
        let g = graph(&[("b", &[]), ("a", &[]), ("c", &["a", "b"])]);
        assert_eq!(publish_order(&g).unwrap(), vec!["a", "b", "c"]);
    }

    #[test]
    fn a_cycle_is_refused_rather_than_ordered() {
        let g = graph(&[("a", &["b"]), ("b", &["a"])]);
        let error = publish_order(&g).expect_err("a cycle has no publish order");
        assert!(error.contains("cycle"), "{error}");
    }

    /// The injection plan 26 asks for: reorder the steps so a crate publishes
    /// before its dependency, and prove something catches it *before* the
    /// first upload.
    #[test]
    fn an_order_that_publishes_a_crate_before_its_dependency_is_refused() {
        let g = graph(&[("facade", &["leaf"]), ("leaf", &[])]);
        let wrong = vec!["facade".to_string(), "leaf".to_string()];
        let problems = validate_order(&wrong, &g).expect_err("facade cannot go first");
        assert_eq!(problems.len(), 1);
        assert!(
            problems[0].contains("facade is published before leaf"),
            "{problems:?}"
        );
    }

    /// A crate dropped from the order is as bad as one in the wrong place, and
    /// much quieter: the release simply never publishes it.
    #[test]
    fn a_crate_missing_from_the_order_is_refused() {
        let g = graph(&[("facade", &["leaf"]), ("leaf", &[])]);
        let short = vec!["leaf".to_string()];
        let problems = validate_order(&short, &g).expect_err("facade is missing");
        assert!(
            problems[0].contains("facade is in the workspace"),
            "{problems:?}"
        );
    }

    /// This repository's own graph has an order, and the facade is late in it.
    #[test]
    fn this_repository_has_a_publish_order() {
        let g = released_graph(&repo_root());
        assert!(g.len() >= 10, "{g:?}");
        let order = publish_order(&g).expect("the crate graph is acyclic");
        validate_order(&order, &g).expect("and its order is legal");
        let facade = order
            .iter()
            .position(|n| n == "tinker-pdf")
            .expect("facade");
        let ffi = order
            .iter()
            .position(|n| n == "tinker-pdf-ffi")
            .expect("ffi");
        let math = order
            .iter()
            .position(|n| n == "tinker-pdf-math")
            .expect("math");
        assert!(math < facade, "{order:?}");
        assert!(facade < ffi, "{order:?}");
    }

    /// The default is a dry run, stated by a test rather than by a comment.
    #[test]
    fn nothing_publishes_unless_execute_is_passed() {
        assert!(!Options::default().execute);
        assert!(!parse(&[]).unwrap().execute);
        assert!(!parse(&["--dry-run".to_string()]).unwrap().execute);
        assert!(parse(&["--execute".to_string()]).unwrap().execute);
    }

    /// The safety property of the whole module, as one assertion: **a dry run
    /// never issues a command that uploads.** Every step that publishes either
    /// has a dry form that differs from its live one, or has none and is
    /// skipped. A step marked `publishes` whose dry form equalled its live
    /// form would publish from `cargo xtask release` with no arguments.
    #[test]
    fn a_dry_run_never_issues_a_publishing_command() {
        let steps = plan(&repo_root(), &Options::default()).expect("a plan");
        assert!(steps.iter().any(|step| step.publishes), "{steps:?}");
        for step in &steps {
            if !step.publishes {
                continue;
            }
            if let Some(dry) = &step.dry {
                assert_ne!(
                    dry, &step.live,
                    "{}: a dry run would run the live publish",
                    step.label
                );
            }
        }
    }

    /// And the four uploads are all there. A step silently dropped from the
    /// plan makes the assertion above pass vacuously for whatever is missing.
    #[test]
    fn every_registry_has_an_upload_step() {
        let steps = plan(&repo_root(), &Options::default()).expect("a plan");
        for stage in [Stage::Crates, Stage::Wheel, Stage::Npm, Stage::NuGet] {
            assert!(
                steps
                    .iter()
                    .any(|step| step.stage == stage && step.publishes),
                "nothing uploads for {}",
                stage.name()
            );
        }
    }

    /// The stages come in plan 13's order and the crates come before every
    /// language package, because each of those builds against the published
    /// facade.
    #[test]
    fn the_stages_run_in_dependency_order() {
        let steps = plan(&repo_root(), &Options::default()).expect("a plan");
        let rank = |stage: Stage| match stage {
            Stage::Preflight => 0,
            Stage::Crates => 1,
            Stage::Wheel => 2,
            Stage::Npm => 3,
            Stage::NuGet => 4,
        };
        let mut last = 0;
        for step in &steps {
            let here = rank(step.stage);
            assert!(here >= last, "{} runs out of order", step.label);
            last = here;
        }
    }

    #[test]
    fn only_selects_one_stage() {
        let options = Options {
            only: vec![Stage::Crates],
            ..Options::default()
        };
        let steps = plan(&repo_root(), &options).expect("a plan");
        assert!(steps.iter().all(|s| s.stage == Stage::Crates));
        assert!(steps.len() >= 10);
    }

    /// The defect this pipeline found on its first run, kept as a test so it
    /// cannot come back: `tinker-pdf-font` excluded the CMap registry its own
    /// build script asserts is present, so the published crate would have
    /// built for nobody.
    #[test]
    fn a_crate_that_vendors_data_publishes_it() {
        if let Err(problems) = vendored_data_reaches_the_package(&repo_root()) {
            panic!("vendored data would not reach the package:\n{problems:#?}");
        }
    }

    /// And the registry is really in there, rather than the check passing on a
    /// crate that happens to have one stray file under `data/`.
    #[test]
    fn the_cmap_registry_is_in_the_font_crates_package() {
        let (listed, bytes) =
            package_contents(&repo_root(), "tinker-pdf-font").expect("cargo package --list");
        let data = listed.iter().filter(|p| p.starts_with("data/")).count();
        assert!(data > 200, "only {data} data files: {listed:?}");
        assert!(
            bytes < CRATES_IO_CEILING * 4,
            "{bytes} bytes uncompressed is close enough to the ceiling to want measuring"
        );
    }

    /// The RID is a NuGet concept and cargo has never heard of it, so the
    /// mapping is the thing that can be wrong — and a wrong RID packs a
    /// library the .NET host will simply never look at, which is a package
    /// that builds, restores, and throws `DllNotFoundException` on first use.
    #[test]
    fn the_host_rid_names_a_library_that_could_exist() {
        let (rid, library) = host_rid().expect("this platform is mapped");
        assert!(
            [
                "win-x64",
                "win-arm64",
                "linux-x64",
                "linux-arm64",
                "osx-x64",
                "osx-arm64"
            ]
            .contains(&rid),
            "{rid}"
        );
        assert!(library.contains("tinker_pdf_ffi"), "{library}");
        // The three platforms use three different shapes, and getting this
        // from `std::env::consts` rather than from an `if cfg!` is what keeps
        // them right.
        match std::env::consts::OS {
            "windows" => assert_eq!(library, "tinker_pdf_ffi.dll"),
            "linux" => assert_eq!(library, "libtinker_pdf_ffi.so"),
            "macos" => assert_eq!(library, "libtinker_pdf_ffi.dylib"),
            other => panic!("unmapped OS {other}"),
        }
        assert_eq!(
            native_path(rid, &library),
            format!("bindings/dotnet/runtimes/{rid}/native/{library}")
        );
    }

    /// The one failure a dry run of a multi-crate release always produces,
    /// and the one that must not be confused with a broken manifest.
    #[test]
    fn an_unpublished_sibling_is_told_apart_from_a_missing_third_party_crate() {
        let released = vec!["tinker-pdf-math".to_string(), "tinker-pdf".to_string()];
        let ours = "error: failed to prepare local package for uploading\n\n\
                    Caused by:\n  no matching package named `tinker-pdf-math` found\n  \
                    location searched: crates.io index\n";
        assert_eq!(
            missing_internal_dependency(ours, &released).as_deref(),
            Some("tinker-pdf-math")
        );
        // Same sentence, and it is somebody's typo. It must still halt.
        let theirs = "Caused by:\n  no matching package named `serde_jsno` found\n";
        assert_eq!(missing_internal_dependency(theirs, &released), None);
    }

    #[test]
    fn a_duplicate_upload_is_recognised_and_a_real_failure_is_not() {
        assert!(is_already_published(
            "error: crate version `0.0.1` is already uploaded"
        ));
        assert!(is_already_published(
            "409 Conflict: package already exists."
        ));
        assert!(!is_already_published(
            "error: failed to verify package tarball\ncould not compile `tinker-pdf`"
        ));
        assert!(!is_already_published("error: no matching package named"));
    }
}
