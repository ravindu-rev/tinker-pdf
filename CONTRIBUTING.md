# Contributing to tinker-pdf

## Build

```bash
cargo build                    # no C toolchain, no bindgen, no fetching at build time
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

That is the whole prerequisite list: `rustup`, and nothing else. Keeping it
that way is a feature, not an accident — see [README](README.md) for what
this engine is.

`python3 -I tools/gates.py` runs the local gates — fmt, clippy, the tests,
the determinism and shaping fingerprints, the examples and `cargo xtask check`
— and prints one line per gate; name gates to run only those (`--list` names
them). CI runs the tests alone, on every pull request, and a merge to `main`
runs them again before it releases; everything else
[docs/verification.md](docs/verification.md) lists is run locally, as a
change's proof. Each gate's whole output is kept in
`target/gates/<gate>.log`, and a gate that fails prints its failing tests,
its first compiler errors and the lines it had to print and did not. It
needs Python 3; the build does not.

Test binaries built with full debug info, and cargo's incremental state, are
large: on a machine short of disk, run the gates with
`CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`.

Two build steps are not `rustc`. `tinker-pdf-font/build.rs` compiles Adobe's
vendored CMap registry (`THIRDPARTY.md`) into static tables. It reads a
directory that is in the repository — nothing is downloaded, so an offline
build and a reproducible one both still work — and the workspace sets
`opt-level = 2` for build scripts so it costs seconds rather than a minute.
`tinker-pdf-color/build.rs` computes the engine's own sRGB ICC profile from
IEC 61966-2-1's published numbers (`docs/design/icc.md`, RC-19). It reads
nothing at all, and its arithmetic is IEEE 754's four operations and
`tinker-pdf-math`'s power, a build-dependency, so every build host writes the
same bytes.
The CLI's `tools/tpdf/build.rs` runs nothing: on Linux it asks the linker to
align `tpdf`'s segments to 64 KiB, for the reason
[docs/verification.md](docs/verification.md) gives under the corpus peaks.

Features are a gate leg, and they cannot be run from the workspace root:

```bash
cargo test -p tinker-pdf-font --no-default-features
cargo test -p tinker-pdf --no-default-features
```

`--workspace --no-default-features` turns nothing off, because `tools/` and
`tinker-pdf-ffi` depend on the facade with its defaults and cargo unifies
features across everything in one build. It passes, and it proves nothing.

`wasm32-unknown-unknown` is a first-class target, built locally as a
change's proof:

```bash
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown -p tinker-pdf
```

Ruling 4 — the same document renders to the same bytes on every target — is
proved by rendering, not by inspection, and wasm is the fourth of the four
targets it names. `wasm32-unknown-unknown` cannot run a test binary without a
JavaScript harness, so the check runs on `wasm32-wasip1`, which is the same
code generation with a `main` a runner can execute:

```bash
rustup target add wasm32-wasip1
# and a wasmtime from https://wasmtime.dev
CARGO_TARGET_WASM32_WASIP1_RUNNER=wasmtime \
  cargo test -p tinker-pdf --test determinism --target wasm32-wasip1
```

The runner is resolved through the `PATH` of the process cargo spawns, so if a
fresh install has not reached your shell yet, give the absolute path rather
than the bare name — the symptom otherwise is cargo trying to execute a
`.wasm` directly, which does not say "wasmtime is missing".

On a Windows host the *linux* leg is a `wsl` away and worth running, because
it covers a different axis from wasm: wasm is the width test, and linux is a
different `std`, allocator, `libm` and linker over the same arithmetic. Copy
the tree onto ext4 — building across `/mnt/c` is glacial — and run the ordinary
command; no target or runner flag is needed, since inside WSL
`x86_64-unknown-linux-gnu` is native.

**If this disagrees with a native run, do not update the fingerprints.** Two
targets disagreeing is a determinism bug; the table is the evidence, and
editing it destroys the only thing in the repository that would ever report
one. `crates/tinker-pdf/tests/determinism.rs` says which of the two failures
you are looking at.

## The rules that are not negotiable

1. **Everything is hand-rolled.** No third-party crate implements any part
   of parsing, filters, crypto, fonts, colour, layout or rasterization. Dev,
   build and binding tooling (proptest, criterion, cargo-fuzz, PyO3,
   wasm-bindgen, maturin, csbindgen, and cachegrind, the instruction counter
   the benches' count mode runs under) is exempt and never ships inside the
   engine. A new dependency on anything else needs a
   [docs/architecture.md](docs/architecture.md) amendment first — the
   boundary is defined there.

2. **Never panic on untrusted input.** No `unwrap`, `expect` or unchecked
   indexing on anything derived from document bytes; `unwrap` is allowed only
   for a provable invariant, with a comment saying which. No `unsafe` in the
   engine crates. This is ruling 1 in [docs/rulings.md](docs/rulings.md) and
   the fuzzers enforce it — a fuzz crash blocks a release.

3. **Leaf crates stay PDF-free.** A leaf takes bytes and plain parameter
   structs and returns bytes and values. No COS types, no PDF vocabulary in
   their public APIs. That is what keeps them independently fuzzable, testable
   and publishable. This is ruling 8 in [docs/rulings.md](docs/rulings.md),
   and the test of it is the definition rather than a list: if a crate takes
   bytes and returns values, it is a leaf and this rule binds it. The current
   membership is listed in one place, [docs/architecture.md](docs/architecture.md).

4. **The docs describe the code.** Every crate's doc comment names the
   feature doc that describes it ([docs/README.md](docs/README.md) indexes
   them). A change that alters behaviour updates that doc in the same commit
   — a doc that drifts from the code is worse than no doc. Read
   [docs/rulings.md](docs/rulings.md) first: its rulings override
   everything, and a change that reverses one edits it in the same commit.
   Work not yet done lives in [docs/ROADMAP.md](docs/ROADMAP.md) and its
   step files, and, for large items, a design doc in
   [docs/design/](docs/design/).

## Code

Warnings are data, not log lines: every leniency the engine performs emits a
typed, object-addressed warning (ruling 10), so "it opened" and "it opened
cleanly" stay distinguishable. Nothing in a library crate prints.

Comments earn their place by stating a constraint the code cannot show —
overwhelmingly, a spec citation:

```rust
// 7.3.4.2: octal escapes take one to three digits, overflow mod 256.
```

## Tests

Unit tests live beside the code; integration tests in `tests/`. Property
tests use proptest — "never panics on arbitrary bytes" is the standing
property for every parser and decoder.

Fixtures in `testdata/` were generated with mutool (see
[`testdata/README.md`](testdata/README.md)); do not modify them — committed
goldens and parity tests assume their exact bytes.

External corpora are fetched locally with `cargo xtask corpus-fetch`, never
committed. They are *inputs* —
documents real producers emitted — and ruling 13 keeps them for exactly that
reason.

**No test may spawn a program.** Ruling 13: nothing outside this repository
renders, parses, validates or measures a document as evidence.
`cargo xtask oracles` holds that boundary with a build failure, and it is
part of `cargo xtask check`, so a `Command::new` in a test fails the gates
and the release unless its file is listed in `SPAWNERS` with the reason it
may. Every row there is
`PERMANENT` and none adjudicates a document: `rustc` to prove a type refuses
a state, `curl`, `tar` and `unzip` to fetch corpora this repository then
verifies against its own SHA-256, `cargo` to build and publish, the corpus
child, a workspace binary built from the same revision, and a test binary
re-run as a child of itself so that a measured case has a process of its own.
A row whose file has stopped spawning fails the check too.

A skipped test exits 0 and reads exactly like a pass, so every check that
depends on something being present prints a `RAN` banner, and a run without
one is no proof.

## Commits and licensing

Conventional Commits. Contributions are accepted under the project's dual
**MIT OR Apache-2.0** license; by contributing you agree your work ships
under both. Dependencies must fit `deny.toml`'s allowlist, which contains no
copyleft at all and is meant to stay that way.
