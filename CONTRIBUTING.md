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

One build step is not `rustc`: `tinker-pdf-font/build.rs` compiles Adobe's
vendored CMap registry (`THIRDPARTY.md`) into static tables. It reads a
directory that is in the repository — nothing is downloaded, so an offline
build and a reproducible one both still work — and the workspace sets
`opt-level = 2` for build scripts so it costs seconds rather than a minute.

Features are a gate leg, and they cannot be run from the workspace root:

```bash
cargo test -p tinker-pdf-font --no-default-features
cargo test -p tinker-pdf --no-default-features
```

`--workspace --no-default-features` turns nothing off, because `tools/` and
`tinker-pdf-ffi` depend on the facade with its defaults and cargo unifies
features across everything in one build. It passes, and it proves nothing.

`wasm32-unknown-unknown` is a first-class target and CI builds it on every
push:

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

Four of them. Each exists because breaking it costs more later than it saves
now, and reviews enforce all four.

1. **Everything is hand-rolled.** No third-party crate implements any part
   of parsing, filters, crypto, fonts, colour, layout or rasterization. Dev,
   build and binding tooling (proptest, criterion, cargo-fuzz, PyO3,
   wasm-bindgen, maturin, csbindgen) is exempt and never ships inside the
   engine. A new dependency on anything else needs a
   [docs/architecture.md](docs/architecture.md) amendment first, not a PR
   comment — the boundary is defined there.

2. **Never panic on untrusted input.** No `unwrap`, `expect` or unchecked
   indexing on anything derived from document bytes; `unwrap` is allowed only
   for a provable invariant, with a comment saying which. No `unsafe` in the
   engine crates. This is ruling 1 in [docs/rulings.md](docs/rulings.md) and
   the fuzzers enforce it — a fuzz crash blocks a release.

3. **Leaf crates stay PDF-free.** `filters`, `crypto`, `font`, `color`,
   `raster`, `math`, `zip`, `xml`, `css` and `layout` — **ten** — take bytes
   and plain parameter structs, return bytes and values. No COS types, no PDF
   vocabulary in their public APIs. That is what keeps them independently
   fuzzable, testable and publishable. This is ruling 8 in
   [docs/rulings.md](docs/rulings.md), and the test of it is the definition
   rather than the list: if a crate takes bytes and returns values, it is a
   leaf and this rule binds it.

4. **The docs are the record.** Every crate's doc comment names the feature
   doc that describes it ([docs/README.md](docs/README.md) indexes them). A
   PR that changes behaviour updates that doc in the same PR — a doc that
   drifts from the code is worse than no doc, because nobody goes looking.
   Read [docs/rulings.md](docs/rulings.md) first — its rulings override
   everything, and a change that reverses one edits it in the same commit.
   Forward-looking work is proposed in [docs/ROADMAP.md](docs/ROADMAP.md)
   and, for large items, a design doc in [docs/design/](docs/design/).

## Working an item

Roadmap items carry **exit criteria** that are deliberately concrete — a
test that runs, a corpus number, a counted injection — and a design doc when
the item is large. Build to the exit criteria and treat a design doc's milestone
table as the commit boundary set.

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

External corpora are fetched, never committed. They are *inputs* — documents
real producers emitted — and ruling 13 keeps them for exactly that reason.

**No new test may spawn a program.** Ruling 13: nothing outside this
repository renders, parses, validates or measures a document as evidence.
`cargo xtask oracles` holds that boundary with a build failure, and it is
part of `cargo xtask check`, so a `Command::new` in a test fails CI unless
its file is listed in `SPAWNERS` with the reason it may — and a row there
whose file has stopped spawning fails too, so an allowance cannot outlive
the thing it allowed.

**Every row is `PERMANENT`, and none of them adjudicates a document.** What is
left spawns `rustc` to prove a type refuses a state, `curl` and `tar` to fetch
corpora this repository then verifies against its own SHA-256, `cargo` to build
and publish, and the corpus child, which is a workspace binary built from the
same revision. The four qpdf rows left with the strict validator, the mutool row
with `xps_conservation.rs`, the browser and epubcheck rows with
`epub_analytic.rs` and `epub_reftest.rs`, and `tools/oracle-diff` was deleted
outright — every allowance in the same commit as the thing it allowed.

The `RAN` / `SKIPPED` discipline outlives the oracles it was written for. A
skip exits 0 and reads exactly like a pass, so every job that depends on
something being present greps its own output for a banner and fails without
one — the fetched corpora and the corpus run's own strict pass included.

## Commits and licensing

Conventional Commits. Contributions are accepted under the project's dual
**MIT OR Apache-2.0** license; by opening a PR you agree your work ships under
both. Dependencies must fit `deny.toml`'s allowlist, which contains no
copyleft at all and is meant to stay that way.
