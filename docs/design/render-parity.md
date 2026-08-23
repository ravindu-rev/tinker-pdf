# Render parity over the corpus

When this is done, a nightly CI job will compare pages this engine drew
against pages an independent renderer drew, over a committed subset of the
corpus, and hold a committed agreement percentage the same way
`corpus/ratchet.json` holds the pass rate — closing the gap
[verification.md](../verification.md) names as its honest limit: as of
August 2026, 2 787 tests prove the engine agrees with itself, the 4 525-file
corpus run proves a bitmap came back, and nothing proves the bitmap is
*right*. The tools already
exist — `tools/oracle-diff` drives `render`/`text`/`which` against mutool,
`pdftoppm` and `pdfium_test` as subprocesses (ruling 9,
[rulings.md](../rulings.md)), and `tools/pdfcmp` gates on the fraction of
changed pixels — what is missing is the wiring: a subset, a budget, a bar,
and a job that goes red when the oracle silently was not there.

## Scope

- A committed parity manifest: which corpus files are compared, against which
  oracle, at what budget, selected for breadth across capabilities.
- A `cargo xtask parity-run` command that drives the comparison over the
  manifest and holds a committed ratchet (`corpus/parity-ratchet.json`) with
  the same `--check`/`--record` discipline as `xtask corpus-run`.
- A nightly CI job in `.github/workflows/corpus.yml` with a
  `SKIPPED`-is-red oracle guard, plus a small per-PR smoke over committed
  fixtures.
- Triage artifacts: `pdfcmp --diff` heat maps and both renders, uploaded on
  every failure.
- A font-provider policy for the parity run that keeps the comparison honest
  while the engine bundles no faces.

## Non-goals

- **Correctness adjudication.** Parity measures *agreement*, not truth.
  `tools/oracle-diff/src/main.rs` says it in its header: two of the defects
  that motivated this engine are cases where the oracle was wrong. A
  disagreement is triaged by a person; the ratchet only stops agreement from
  silently decreasing.
- **Full-document parity.** `oracle-diff render` compares page one only
  (mutool `draw ... 1`, `pdftoppm -f 1 -l 1`, `pdfium_test --pages=0`), and
  this design keeps that: breadth over hundreds of files beats depth in a
  few. Multi-page comparison is future work and the manifest records the
  limit.
- **A `--fonts` parity bar.** That follows the `--fonts` corpus bar already
  on the [roadmap](../ROADMAP.md); phase 1 sidesteps the question (see font
  policy below).
- **Text-extraction parity as a gate.** `oracle-diff text` similarity is
  recorded and ratcheted as a second axis in a later milestone, never a
  per-file hard gate — extractors legitimately differ on ordering and
  hyphenation.

## Design

### Subset selection: breadth per capability

The nightly corpus run already writes a per-file report (`corpus/report.json`
via `xtask/src/report.rs`) in which every `FileResult` (defined in
`xtask/src/runner.rs`) carries `capabilities`
(`jbig2`, `jpx`, `mesh-shading` — ISO 32000-1 7.4.7, 7.4.9, 8.7.4.5) and
`warnings` (ruling 10 provenance labels). Selection reuses that record
rather than inventing a classifier:

- **Every file with a minority capability tag** — the ratchet counts 103
  JBIG2, 19 JPX and 10 mesh-shading files across the corpora; those are
  exactly the pages where a self-consistent renderer can be consistently
  wrong.
- **A deterministic sample of clean files** (no warnings) per corpus, in
  path order like `corpus-run --sample`, capped so the whole run stays in
  minutes: roughly 300 files total at 150 dpi.
- **Phase-1 exclusion:** any file whose report carries a missing-face
  warning (see font policy).

`cargo xtask parity-select --from corpus/report.json` writes the manifest to
`corpus/parity.json`: corpus name, relative path, reason tag
(`capability:jbig2`, `sample`), and any per-file budget override with a
mandatory reason string, in the style of `bounds_ledger.rs`. The manifest is
committed and reviewed; `parity-select --check` fails when it drifts from
the report, exactly as `corpus-licences --check` ties `corpus/README.md` to
the lock.

### The metric and per-oracle budgets

The comparison is `pdfcmp`'s and only `pdfcmp`'s — `oracle-diff` already
shells out to it so the metric is defined in one place: the fraction of
pixels where any channel moves more than the threshold (default 12), never
the mean, for the shifted-glyph reason documented in
`tools/pdfcmp/src/main.rs`. Budgets are per oracle, recorded in the
manifest, because anti-aliasing, hinting and colour conversion differ
legitimately between engines and differ *differently* per oracle.
`oracle-diff`'s cross-engine default (`--budget 0.02`, handed to `pdfcmp`,
which applies it to the changed-pixel fraction) is the starting point for
each; tightening a budget is a recorded,
diffed edit like any ratchet move.

### The parity ratchet

`corpus/parity-ratchet.json` is a sibling of `corpus/ratchet.json`, not an
extension of it — its settings differ (150 dpi, an oracle name and version)
and `corpus-run`'s refusal logic must not have to know about it. Same
schema discipline: **counts, never rates** — `agreed` and `compared` per
corpus per oracle, compared by the integer cross-multiplication already
implemented as `ratchet::holds` in `xtask/src/ratchet.rs`
(`agreed_now * compared_before >= agreed_before * compared_now`), reused,
not reimplemented. Same refusal discipline (rulings 2/10): a comparison
whose sides are not the same measurement is refused with a message, not
resolved — an incomplete subset (manifest file missing on disk), a dpi or
fonts mismatch, and an **oracle version mismatch**: the ratchet records the
oracle's `--version` string, the workflow pins the installed package, and
bumping the pin means re-recording the bar in the same PR. A timed-out or
crashed oracle invocation is likewise a refusal, never counted as a
disagreement. `parity-run --check` and `--record` are mutually exclusive,
as in `RunArgs::parse`.

### The oracle guard: SKIPPED is red

`oracle-diff` deliberately treats a missing oracle as skippable so a
developer can run it at all; CI must not. The job prints
`parity-oracle: RAN <name> <version>` or `parity-oracle: SKIPPED` and the
workflow greps for `SKIPPED` first, by name, then requires `RAN` — the
exact pattern `.github/workflows/ci.yml` uses for `qpdf-oracle:`,
`mutool-oracle:` and `jpx-oracle:`, established by measurement (removing
qpdf from `PATH` and watching the suite pass, per
[verification.md](../verification.md)). `oracle-diff which` runs first so
the log names what was installed.

### Where the job runs

**Nightly**, as a job in `.github/workflows/corpus.yml`: it needs the
fetched corpora, an installed oracle, and minutes of rendering — none of
which belongs between a push and a review, per that workflow's own header.
**Per-PR**, a smoke step in `ci.yml`: `oracle-diff render` over the
committed render-fingerprint fixtures of
`crates/tinker-pdf/tests/determinism.rs` (small, in-repo, no fetch), with
its own `parity-smoke: RAN` guard — so the wiring itself cannot rot for a
month between nightly failures.

### Triage: a number that fails without a picture wastes a morning

On any file over budget, the job re-invokes `pdfcmp` with `--diff` and keeps
both renders via `oracle-diff --keep`, then uploads ours/theirs/heat-map
plus the per-file parity report as a `parity-failures` artifact with
`if: always()` and a 14-day retention — the same shape as the existing
`corpus-report` upload. The report names the file, the oracle, the
changed-pixel fraction against its budget, and the worst pixel's location,
which `pdfcmp` already prints.

### Font-provider policy

The engine bundles no faces; `corpus/ratchet.json` was measured with
`fonts: "none"` and its note says degradation is dominated by that policy.
An oracle *does* substitute faces for unembedded fonts (ISO 32000-1 9.6,
9.7), so on such a file the comparison measures font policy, not rendering.
Phase 1 therefore compares only files with **no missing-face warning** —
both engines draw from the same embedded font programs — and the manifest
records the exclusion as a named limit, the way `ratchet.json`'s note names
its own. Phase 2, after the roadmap's `--fonts` corpus bar exists, adds a
second ratchet measured with a pinned face directory passed to both `tpdf`
(via the existing `FontProvider` seam in `crates/tinker-pdf/src/fonts.rs`)
and recorded in settings; the comparator refuses to mix the two bars, as
`corpus-run` already refuses today.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size (S/M/L/XL) |
| --- | --- | --- | --- |
| 1 | `xtask parity-select` and the committed `corpus/parity.json` | Manifest covers all four corpora and every capability tag with a nonzero count in `ratchet.json`; excludes every file with a missing-face warning; `parity-select --check` exits nonzero when the manifest drifts from `corpus/report.json` (proven by deleting one row and running it) | S |
| 2 | `xtask parity-run` with `--check`/`--record` and `corpus/parity-ratchet.json` | Injection: hand-raising `agreed` in the committed ratchet makes `parity-run --check` exit nonzero with a `regression:` line; a dpi, fonts, or oracle-version mismatch produces a `refused:` line naming the field; unit tests on the comparator reuse `ratchet::holds` | M |
| 3 | Nightly parity job in `corpus.yml` with guard and artifacts | Job log contains `parity-oracle: RAN`; a scratch run with the oracle removed from `PATH` goes red on the `SKIPPED` grep, not green; a forced over-budget file yields a `parity-failures` artifact containing the heat map | S |
| 4 | Per-PR smoke in `ci.yml` over committed fixtures | Step runs `oracle-diff render` on the determinism fixtures and greps `parity-smoke: RAN`; red when the oracle is absent | S |
| 5 | Text-agreement axis in the parity ratchet | `parity-ratchet.json` carries per-corpus text `agreed`/`compared` counts from `oracle-diff text`; injection of a lowered count fails `--check` | S |

## Dependencies

- `tools/oracle-diff` and `tools/pdfcmp` as they stand (no metric changes);
  `oracle-diff` grows only a machine-readable per-file result line for
  `parity-run` to consume.
- `xtask` plumbing: `ratchet::holds`, `report.rs`/`json.rs` serialization,
  `corpus.rs::pdfs_under`, the lockfile and `corpus-fetch`.
- One pinned oracle package installed in the nightly workflow (mutool or
  `pdftoppm`; the manifest names which).
- The `--fonts` corpus bar ([ROADMAP.md](../ROADMAP.md)) — for phase 2
  only; phase 1 does not wait on it.
- Committed fixtures of `crates/tinker-pdf/tests/determinism.rs`
  ([features/determinism.md](../features/determinism.md)) for the per-PR
  smoke.

## Risks

| Risk | Mitigation |
| --- | --- |
| The oracle is the one that is wrong, and the ratchet enshrines its bug | Parity is agreement, not correctness; a per-file budget override with a mandatory reason string records the adjudicated cases in the manifest, reviewed in diff |
| Oracle version drift moves pixels and fails runs nobody broke | The package is pinned in the workflow; the ratchet records the `--version` string and refuses a mismatched comparison; a pin bump re-records the bar in the same PR |
| Budgets too loose to catch anything, or too tight to stay green | Injection, house-style: shift a glyph in a fixture and count whether the gate fires; start at 0.02 per oracle and tighten by recorded, diffed edits with heat maps as evidence |
| The subset rots as corpora grow or capabilities land | `parity-select --check` runs in the nightly job before `parity-run`, failing when the manifest no longer matches the report |
| Missing-face exclusion quietly shrinks the subset toward triviality | The manifest records excluded counts per corpus as a named limit; milestone 1's exit pins capability coverage, so an empty capability bucket is a check failure |
| Oracle subprocess hangs or crashes mid-run | The per-file timeout discipline of `xtask/src/runner.rs` applies to oracle invocations; a timeout is a refusal in the report, never a counted disagreement, so it cannot lower the bar unnoticed |
