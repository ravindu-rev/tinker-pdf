# Contributing to tinker-pdf

## Build

```bash
cargo build                    # no C toolchain, no bindgen, no fetching at build time
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

That is the whole prerequisite list: `rustup`, and nothing else. Keeping it
that way is a feature, not an accident — see [README](README.md) on why this
engine exists.

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

1. **Everything PDF is hand-rolled.** No third-party crate implements any part
   of parsing, filters, crypto, fonts, color or rasterization. Dev, build and
   binding tooling (proptest, criterion, cargo-fuzz, PyO3, wasm-bindgen,
   maturin, csbindgen) is exempt and never ships inside the engine. A new
   dependency on anything else needs a plan-file amendment first, not a PR
   comment. The boundary is defined in
   [`docs/plans/00-architecture.md`](docs/plans/00-architecture.md).

2. **Never panic on untrusted input.** No `unwrap`, `expect` or unchecked
   indexing on anything derived from document bytes; `unwrap` is allowed only
   for a provable invariant, with a comment saying which. No `unsafe` in the
   engine crates. This is ruling 1 in
   [`docs/plans/99-consistency.md`](docs/plans/99-consistency.md) and the
   fuzzers enforce it — a fuzz crash blocks a release.

3. **Leaf crates stay PDF-free.** `filters`, `crypto`, `font`, `color`,
   `raster`, `math`, `zip` and `xml` — **eight** — take bytes and plain
   parameter structs, return bytes and values. No COS types, no PDF vocabulary in their
   public APIs. That is what keeps them independently fuzzable, testable and
   publishable. This is ruling 8 in
   [`docs/plans/99-consistency.md`](docs/plans/99-consistency.md), and the
   test of it is the definition rather than the list: if a crate takes bytes
   and returns values, it is a leaf and this rule binds it.

4. **The plan is the spec.** Every crate's doc comment names the plan file
   that governs it. Implement to the plan; where reality disagrees, edit the
   plan in the same PR and say why. Read
   [`docs/plans/99-consistency.md`](docs/plans/99-consistency.md) first — its
   rulings override the phase plans.

## Working a phase

One plan file per effort. Read it end to end, build to its **exit criteria**
(they are deliberately concrete — a test that runs, a corpus number, a fuzz
campaign), and treat the milestone table as the commit boundary set.

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

Fixtures in `testdata/` are copies of Tinker's, generated by MuPDF (see
[`testdata/README.md`](testdata/README.md)) — the parity bar this engine is
measured against. Do not modify them; until phase 12 the engine cannot
generate its own.

External corpora are fetched, never committed, and oracles (mutool, pdftoppm,
pdfium_test, qpdf) are CI subprocesses, never dependencies. The reasoning is
recorded once in
[`docs/plans/14-testing-and-corpora.md`](docs/plans/14-testing-and-corpora.md).

One oracle runs today: `crates/tinker-pdf-cos/tests/qpdf_oracle.rs` puts
`qpdf --check` and `qpdf --show-linearization` over the linearized writer's
output. Without qpdf on `PATH` those tests **skip**, and a skip exits 0 and
reads exactly like a pass — so the CI job greps its own output for
`qpdf-oracle: RAN` and fails if it finds `qpdf-oracle: SKIPPED` instead. Set
`TINKER_QPDF` to an absolute path if your install is somewhere `PATH` does not
reach; quote it if the path has spaces.

## Commits and licensing

Conventional Commits. Contributions are accepted under the project's dual
**MIT OR Apache-2.0** license; by opening a PR you agree your work ships under
both. Dependencies must fit `deny.toml`'s allowlist, which contains no
copyleft at all and is meant to stay that way.
