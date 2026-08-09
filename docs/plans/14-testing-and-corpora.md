# Phase 14 — Testing and corpora

This is the doctrine every other phase cites, written once so sixteen plans do
not each half-describe it. When this phase's first milestone is done, the
harnesses exist; the rest of it is ongoing practice that never closes. The
posture in one sentence: **correctness claims are executable** — parity is a
`cargo test`, leniency is a fuzz target that never crashes, render quality is
a perceptual budget in CI, and coverage of the real world is a nightly
dashboard, not an impression.

## Scope

- Differential oracles (mutool, pdftoppm, pdfium_test, qpdf) as CI
  subprocesses.
- External corpora: fetch machinery, per-corpus licensing posture, CI wiring.
- Fuzzing: per-format targets, schedules, corpus management, OSS-Fuzz.
- `pdfcmp`: the canonical perceptual comparator.
- Nightly dashboards: corpus pass-rates and capability hit-rates.
- Crypto vectors, determinism checks, wasm smoke, and `tinker_parity.rs`.

## Non-goals

- Performance benchmarking methodology — lives with the hardening milestone
  in [00-architecture](00-architecture.md) (criterion, exempt tooling).
- Tinker's own test suite — it stays in Tinker; we port it
  (assertion-for-assertion, ruling 12) rather than import it.

## Design

### Oracles are subprocesses, never dependencies

`tools/oracle-diff` invokes `mutool draw`/`mutool show`, `pdftoppm`,
`pdfium_test` and `qpdf` as external CLIs in CI, compares their output to
ours, and discards it. The licensing reasoning, recorded once here (ruling
9): running an AGPL program as a subprocess and reading its output creates no
derivative work and no linkage — AGPL obligations attach to distributing or
network-serving *that program*, which we never do; the binaries are installed
from distro packages inside CI and their outputs are transient comparison
references, never committed, never redistributed, never shipped. pdfium
(BSD-3) and qpdf (Apache-2.0) are looser still. What we must *not* do is
equally recorded: no linking mutool code, no vendoring its source, no
shipping its rendered output as our fixtures.

Comparison modes: text (normalized whitespace, similarity threshold), render
(`pdfcmp` budget), structure (`qpdf --check` / `--show-linearization` as
pass-fail validators of our *outputs*).

### Corpora: fetched, pinned, never committed

`corpus/` holds fetch scripts and a lockfile of upstream commits + sha256
per corpus; CI restores from the actions cache keyed on that lockfile.
Nothing from any corpus enters git — size aside, redistribution rights are
per-file murky in every real-world PDF collection, and a pin + checksum
reproduces the set without us becoming a distributor.

| Corpus | What it exercises | License posture | Use |
| --- | --- | --- | --- |
| pdf.js `test/pdfs` | Decades of real-world breakage | Mixed; many files are linked, not stored, by upstream itself | Fetch in CI only; never redistribute |
| veraPDF corpus | Spec-conformance edge cases, validation | Permissive (CC) per upstream | Fetch in CI; safe to cache |
| qpdf qtest suite | Xref/object-stream/encryption torture | Apache-2.0 | Fetch in CI; individual files may be copied into `testdata/` with attribution when a regression needs a committed repro |
| PDF Association samples | Feature showcases (2.0, tagged, collections) | Varies per file — check before any copy | Fetch in CI only |

PR CI runs a curated ~200-file subset (chosen for breadth per capability);
the nightly job runs everything.

### Fuzzing

One cargo-fuzz target per leaf input format: `inflate`, `lzw`, `jpeg`,
`ccitt`, `cff`, `truetype`, `type1`, `cmap`, `cos_parser`,
`content_tokenizer` — each lands in the same PR as its parser, not after.
Short runs (minutes) on every PR; long runs nightly; minimized corpora are
committed (they are ours, tiny, and the regression seeds). A fuzz crash is a
release blocker (ruling 1 — never-panic is fuzz-*enforced*, not aspirational).
OSS-Fuzz application once the repo is public.

### pdfcmp

Our perceptual comparator, and deliberately the same metric shape as Tinker's
`visual_regression.rs`: fraction of pixels where any channel moves more than
a threshold (Tinker: threshold 12, tolerance 0.0005), so every budget Tinker
has already calibrated transfers at integration unchanged. It is the
canonical comparator because it runs everywhere including wasm; `dssim` (AGPL)
is permitted as an optional CI-only subprocess cross-check under the same
reasoning as the oracles, and nowhere else. Failure output: side-by-side plus
per-pixel heat map written to the job's artifacts — a number that fails
without a picture wastes a human's morning.

### Dashboards and ratchets

The nightly corpus job publishes two artifacts:

- **Pass-rate per capability** (opens, text extracted, rendered under
  budget, round-trips) — with a ratchet: the recorded rate may not decrease;
  improving it updates the recorded floor in the same PR.
- **Hit-rate per deferred capability** (JBIG2, JPX, arithmetic JPEG, mesh
  shadings, full ICC) — how many corpus pages actually need each. This is
  the evidence feed for ruling 3: deferred work is scheduled when the number
  says so, not when it looks interesting.

### The fixed suites

- **Crypto vectors**: NIST CAVP (AES-CBC, SHA-2), RFC 6229 (RC4), RFC 1321
  (MD5), fetched by `xtask`, committed parsed (they are tiny and license-free
  facts); merge gates for [03-encryption](03-encryption.md).
- **Determinism**: golden bitmaps compared byte-exact across all four CI
  targets (ruling 4); any divergence fails the matrix, not one leg.
- **wasm smoke**: per-PR `wasm-bindgen-test` opens, authenticates, extracts
  and renders the four fixtures under `wasm32-unknown-unknown`.
- **`tinker_parity.rs`** in the facade crate: assertion-for-assertion ports
  of Tinker's five test files (`open_documents`, `render_pages`,
  `text_and_search`, `outline`, `visual_regression`) against the fixtures in
  `testdata/` (ruling 12). When it is green, "parity" is a fact with a exit
  code, and [15-tinker-integration](15-tinker-integration.md) can begin.

## Milestones

| # | Deliverable | Exit criteria | Size |
| --- | --- | --- | --- |
| 14.1 | Corpus fetch + lockfile + CI cache; oracle-diff runner; pdfcmp | Nightly job runs the full corpus and publishes both dashboards; PR subset wired | S |
| 14.2 | Fuzz harness conventions + first three targets (cos, inflate, lzw) | PR-short + nightly-long schedules live; a seeded crash demonstrably blocks merge | S |
| — | Everything else | Arrives with its phase (targets with parsers, vectors with 03, parity file grows with 04/06/08) | ongoing |

## Dependencies

14.1 needs only the scaffold and [01](01-cos-and-object-model.md)'s first
parser to have something to run. Every other phase depends on this one's
machinery for its exit criteria — which is why it is milestone-numbered
almost first and sized almost smallest.

## Risks

| Risk | Mitigation |
| --- | --- |
| Corpus upstream disappears or rewrites history | Lockfile pins commit + sha256; CI cache retains the last good fetch; mirrors are a one-line lockfile change |
| Oracle version drift changes reference output | Oracle versions pinned in CI images; diffs are perceptual/threshold-based, not exact, so patch-level drift is absorbed |
| Ratchet gamed by lowering budgets | Budget changes require touching this file's successor data in a reviewed PR; the ratchet floor and the budget live in-repo, diffable |
| Fuzz corpus bloat | Minimization on every nightly run; corpora capped per target; only minimized seeds committed |
