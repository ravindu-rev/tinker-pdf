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

## As built — 16 August 2026

Six commits, one per milestone. 1 457 tests to 1 479; the full nine-command
gate green on each, and the eleven determinism fingerprints did not move —
this is packaging, not pixels.

**Nothing was published to any registry.** Not crates.io, not PyPI, not npm,
not NuGet. `cargo publish` was only ever run with `--dry-run`; `maturin
publish`, `npm publish` and `dotnet nuget push` were never run at all. The
non-goal still holds: the facade is not frozen until 0.1.0, and the point of
this work is that the pipeline exists and has been exercised.

### Which absences were real

Five listed, five real — but two of them for a different reason than the plan
gives, and the difference matters to whoever reads the manifests next.

| Plan says | Found |
| --- | --- |
| No abi3 wheel build | The **configuration** was already there: `bindings/python/Cargo.toml` asks pyo3 for `abi3-py39`. What was absent was any build of it, anywhere, ever |
| No per-RID native layout for NuGet | `TinkerPdf.csproj` already carried `<None Include="runtimes/**/*" Pack="true">`. The directory it names had never existed, and `dotnet pack` over an empty one produces a **valid managed-only package** |
| No npm publish, no ESM/CJS decision | Both true. The decision is now recorded in `bindings/js/README.md` |
| No version-synchronisation check | True. Four manifests, and nothing comparing them |
| No `publish = false` on the tools | True for `xtask`, `tpdf`, `pdfcmp` and `oracle-diff`. The fuzz crate and both binding crates already had it |

### Three defects the dry run found, which is what a dry run is for

**`tinker-pdf-font` would have published a crate nobody could build.** It
carried `exclude = ["data/"]`, on the reasoning that Adobe's CMap registry is
compiled by `build.rs` rather than read at run time. True, and beside the
point: build time is *the consumer's* build time, and `build.rs` asserts the
registry is present. Reproduced by moving the directory aside — `no CMaps
under ... the vendored data is missing`, which is what every downstream build
would have printed. Four crates would already have been on crates.io by then.
It ships now: 8.0 MB of PostScript, 2.4 MB compressed, against a 10 MiB
ceiling.

**Every `[workspace.dependencies]` entry needed a `version` beside its
`path`.** Without one, `cargo publish` refuses every crate in the workspace
outright — a path means nothing to whoever downloads the result.

**`cargo xtask check` is not a command this repository defines.** There is no
cargo alias, only `cargo run -p xtask --`. The first end-to-end dry run died
at step 1 of 20 on `no such command: xtask`.

And one that is not a packaging defect at all: `Command::new("wasm-pack")`
cannot start an npm-installed tool on Windows, because those are `.cmd` shims
and Rust appends only `.exe`. Step 15 of 20 reported "program not found" for a
program plainly on `PATH`.

### The full dry run

`cargo run -p xtask -- release --tag v0.0.1`, 20 steps: **11 ran, 1 skipped, 8
unprovable without publishing.** Exit 0, nothing uploaded.

| Stage | Steps | What happened |
| --- | --- | --- |
| preflight | 2 | `xtask check` and `cargo test --workspace`, both real |
| crates | 11 | `cargo publish --dry-run` per crate, bottom-up: crypto, filters, math, color, font, raster, cos, content, render, tinker-pdf, ffi. The **three leaves packaged and verified for real**; the other eight are unprovable, below |
| wheel | 1 | `maturin build --release`, producing a real abi3 wheel |
| npm | 2 | `wasm-pack build`, then `npm publish --dry-run` |
| nuget | 4 | `cargo build -p tinker-pdf-ffi`, `xtask nuget-stage`, `dotnet pack`, then `dotnet nuget push` **skipped** — it has no dry-run form and nothing harmless to substitute |

**What a real run would publish:** eleven crates to crates.io in that order,
then one abi3 wheel per platform to PyPI, then `tinker-pdf-js` to npm, then
`TinkerPdf.0.0.1.nupkg` to nuget.org.

**The one thing a dry run cannot do, stated rather than hidden.** `cargo
publish --dry-run` resolves against the live index, so the eight crates above
the leaves fail with `no matching package named X` — a sentence that reads
exactly like a broken manifest and is not one. It is recognised **narrowly**:
only when not executing, and only when `X` is a crate this same release
publishes. A third-party crate missing under the identical sentence still
halts the chain, and a test pins both directions.

### Building against the published facade, and how that would be verified

Plan 26 chooses "build the language packages against the published crate,
because it proves the published crate works". In dry-run mode that switch
cannot be made — there is no 0.0.1 on crates.io to switch to — so every
affected step prints what it would do rather than pretending it did it. A real
run would rewrite each binding manifest's `tinker-pdf` dependency from
`{ path = ... }` to `{ version = "0.0.1" }` after the crates stage, so
`maturin`, `wasm-pack` and `cargo build -p tinker-pdf-ffi` all resolve from
the registry. Verifying that without publishing needs a registry to publish
*to*: a `dir` registry, or a `[source]` replacement pointed at a local one,
would exercise the whole chain honestly. That was not built here, and it is
the largest remaining hole in the dry run.

### What was verified on this machine, and what was not

This is **Windows 11, x86_64-pc-windows-msvc**. Every row below is either
"observed here" or "written and never run". Nothing is in between.

| Leg | State |
| --- | --- |
| The version check, and every injection against it | **Observed.** Eight defects injected, eight caught, each naming the file |
| `cargo publish --dry-run`, the three leaf crates | **Observed**: packaged and verified |
| `cargo publish --dry-run`, the eight crates above them | **Cannot run** without publishing. Reported as unprovable, not skipped |
| abi3 wheel built, installed, rendered | **Observed**, on `win_amd64` only. One `cp39-abi3` wheel built by CPython 3.12, installed into a 3.12 venv *and* a 3.13 venv, 1190 inked pixels from each |
| Linux and macOS wheels | **Never run.** The matrix exists in `release.yml`; no tag has triggered it |
| npm packed, installed, rendered in Node | **Observed.** `npm pack`, `npm install` of the tarball, 1190 inked pixels |
| The `Uint8Array` invalidation contract | **Observed.** A view goes to **zero length** after an allocation grows the heap — the buffer detaches, it does not dangle |
| NuGet packed, `PackageReference`d from a folder feed, rendered | **Observed on win-x64 only.** The native library resolved out of `runtimes/win-x64/native/`, 1190 inked pixels |
| The linux-x64 and osx-arm64 halves of the NuGet package | **Never run.** `release.yml` builds them on three runners and greps the `.nupkg` for all three RIDs; no tag has run it |
| The browser demo | **Observed**, in headless Chromium driven by Playwright. 2385 non-white pixels read back off the canvas with `getImageData`, in a bounding box of 108,130 to 412,155 on an 893x1263 canvas |
| One tag produces all four artefacts | **Never run.** It cannot be verified without CI observing a tag. `release.yml` is written and unobserved |

The three ecosystems that rendered all produced **1190 inked pixels** on the
same fixture at the same scale. That agreement is worth more than any of the
three numbers alone: it says the bindings are projections of one engine rather
than three things that each drew something.

### The release job is written to prove it ran

`wasm-determinism` and `qpdf-linearization` are both written that way, because
`cargo test` with a filter matching nothing exits 0. A release workflow is the
same hazard with worse consequences: four green jobs and no artefacts looks
exactly like a working release. So:

- the wheel count must be **one** per platform and its filename must carry
  `abi3` — without that check the job passes on a per-interpreter wheel, which
  is the status quo this gap plan calls wrong;
- the `.nupkg` is grepped for **all three** RIDs, because `dotnet pack` on an
  empty `runtimes/` produces a package that restores, compiles, and throws
  `DllNotFoundException` on first use;
- every smoke test prints a token — `WHEEL-SMOKE: RAN`, `NODE-SMOKE: RAN`,
  `DOTNET-SMOKE: RAN`, `DEMO-RENDER: RAN` — and the job greps for it;
- every `upload-artifact` sets `if-no-files-found: error`, because the default
  is `warn` and a warning is a green tick;
- a final `artefacts` job downloads everything and **counts** it: three wheels,
  three of them abi3, one wasm, one nupkg.

A tag publishes nothing. The `publish` job needs a deliberate
`workflow_dispatch` with `publish: true`, and it is one command —
`cargo run -p xtask -- release --execute` — because re-expressing the ordering
in YAML would be a second implementation of the rule that decides which crate
uploads first, and two implementations disagreeing is not recoverable.

### Injection

Every defect injected, re-run, and the catching assertion recorded.

| Injected | Caught by | What it says |
| --- | --- | --- |
| `.csproj` one version behind | `xtask check` | names the file, both versions, and refuses to guess which is wrong |
| `bindings/js` one version ahead | `xtask check` | the same |
| `bindings/python` one version behind | `xtask check` | the same |
| `publish = false` deleted from `xtask` | `xtask check` | names the crate *and why it must not publish* |
| `publish = false` deleted from `tools/tpdf` | `xtask check` | the same |
| `publish = false` **added** to the facade | `xtask check` | names the consequence: the release skips it and the *next* crate fails with an error about something else |
| a `[workspace.dependencies]` version left behind | cargo first, `xtask check` second | cargo refuses to build at all, with `location searched:` and no hint that a version is the problem. The check's message is the readable one, and it is what catches a *loose* requirement cargo would happily accept |
| `pyproject.toml` given a literal version | `xtask check` | names the fifth place a version could drift |
| the publish order reversed | `validate_order`, before any command runs | names which crate precedes which dependency, and **how many uploads would already have been irreversible** |
| a crate dropped from the order | `publish_order`'s readiness guard | the first draft called this "a cycle", which is a wrong lead; it now names both possible causes |
| `exclude = ["data/"]` restored on the font crate | two tests in `cargo test` | `a_crate_that_vendors_data_publishes_it` fails, and `the_cmap_registry_is_in_the_font_crates_package` reports **0 data files** |
| `runtimes/` emptied before `dotnet pack` | the smoke test, at run time | `System.DllNotFoundException: Unable to load DLL 'tinker_pdf_ffi'` (0x8007007E). The package packs and restores perfectly. CI catches it one step earlier, by grepping the `.nupkg` |

Two blind spots the injections exposed while the tests were being written,
both the same shape as PRE-A:

- The **first draft of the Python smoke test asserted only that a bitmap of
  the right size came back**, and it passed at **zero ink**.
  `testdata/simple-text.pdf` embeds no font program and this engine bundles no
  faces, so it renders to a correctly sized blank page — and the smoke test for
  every binding would have passed on a build whose renderer did nothing at all.
  All three now assert the render *twice*, blank and then inked, which also
  exercises the `set_fonts` seam that every real host must call.
- The **first draft of the "no dry-run step publishes" test** asserted that a
  step's dry command differs from its live one. That is false for preflight and
  for the three build steps, and it failed immediately. `Step::publishes` now
  carries the property and the assertion is over the steps that upload.

### Left undone

- **A local registry.** It would let both the eight unprovable crates and the
  build-against-the-published-facade rewrite be exercised without touching
  crates.io.
- **Signing and provenance**, which this plan already lists as a non-goal.
- **`--skip-existing` and `--skip-duplicate` are declared and untested.**
  Resumability is read out of cargo's duplicate-upload error, and that string
  has never been seen here, because nothing has been published. The matcher is
  tested against transcribed text, which is not the same thing.
