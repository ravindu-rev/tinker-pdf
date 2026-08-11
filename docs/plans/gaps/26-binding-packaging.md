# Four language bindings build and none ships

C ABI, Python, JavaScript and .NET all compile, are checked in CI, and can
each supply fonts to the engine. None of them can be installed by anybody.
There is no wheel, no npm package, no NuGet package, no crates.io release, and
no single tag that produces all four. When this is done, `pip install`,
`npm install`, `dotnet add package` and `cargo add` work. (M)

## What is wrong

Plan 13 specifies the shape: a one-tag release through xtask — cargo publish,
then maturin, then wasm-pack, then dotnet pack. None of it exists. There is no
release workflow beyond the versioned-release-on-merge job, and that publishes
nothing to a registry.

Specific absences:

- No abi3 wheel build, so Python needs one wheel per interpreter version
  rather than one per platform.
- No per-RID native library layout for NuGet, which is how a .NET package
  carries platform binaries.
- No npm publish, and no decision recorded about the ESM/CJS split.
- No version synchronisation check, so the four package manifests can drift
  from the workspace version silently.
- No `publish = false` on the crates that should never reach crates.io — the
  tools, xtask, the fuzz crate.

## Scope

- `cargo xtask release`: version check, then publish in dependency order.
- abi3 wheels via maturin, for the platforms the CI matrix already builds.
- wasm-pack to npm, ESM.
- NuGet with the per-RID runtimes layout over the `tinker-pdf-ffi` cdylib.
- A version-synchronisation check that fails when any manifest disagrees with
  the workspace version.
- `publish = false` where publishing would be wrong.
- The browser demo plan 13 names as its exit criterion: a page that renders an
  uploaded PDF.

## Non-goals

- **Publishing before the API freezes.** Plan 00 freezes the facade at 0.1.0
  after Checkpoint B. Publishing 0.0.x is fine and expected; the point of this
  work is that the pipeline exists and is exercised, not that anyone should
  depend on it yet.
- **Signing and provenance attestation.** Worth doing, separate decision, and
  it does not block a first publish.

## Design

**Order is forced by dependencies.** The Rust crates publish bottom-up —
crates.io rejects a crate whose path dependency is unpublished — and the three
language packages then build against the published facade or against the
workspace, which is a choice worth recording: building against the workspace
keeps a release atomic, building against the published crate proves the
published crate works. Prefer the second for the same reason the tools depend
on the facade rather than reaching past it.

**Dry-run first.** Every step gets a dry-run mode, and the first exercise of
the whole pipeline is a dry run end to end. A half-published release cannot be
retracted from crates.io.

**Version synchronisation is a check, not a script that rewrites.** Four
manifests plus the workspace; a mismatch fails rather than being silently
corrected, because a silent correction hides which one was wrong.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | `publish = false` where it belongs; version-sync check | `cargo xtask check` fails when a binding manifest disagrees with the workspace version | S |
| 2 | `cargo xtask release --dry-run`, full pipeline | A dry run reports every step in dependency order without publishing anything | S |
| 3 | abi3 wheels | A wheel installs on the CI matrix's platforms and imports; one wheel per platform, not per interpreter | M |
| 4 | npm package via wasm-pack | `npm install` then a render in Node; the `Uint8Array`-invalidated-on-growth contract documented as plan 13 requires | M |
| 5 | NuGet with per-RID runtimes | `dotnet add package` then a render on Windows and Linux | M |
| 6 | One-tag release; browser demo | A tag produces all four artefacts; the demo renders an uploaded PDF | S |

## Dependencies

**Needs first:** nothing technically. Worth doing after the engine is closer
to parity, since a published package invites use.

**Unblocks:** anyone outside this repository using the engine at all.

## Risks

| Risk | Mitigation |
| --- | --- |
| A half-published release cannot be undone | Dry-run mode first, and the full pipeline exercised as a dry run before any real publish |
| The bindings drift from the workspace version and ship mismatched | Milestone 1's check, in the same xtask that already enforces the crate graph |
| Publishing invites dependence on an API that is explicitly unstable until 0.1.0 | Version numbers say so; the README should too |
