# No corpus has ever been run

Eight real files have been through `tpdf`. The pinned public corpora — pdf.js,
veraPDF, qpdf's qtest, the PDF Association samples — never have. This is the
single largest gap between "tests pass" and "handles what exists", and it is
the gate on ruling 3: three other plans here are waiting for evidence that
does not exist. When this is done, every commit is measured against thousands
of real documents and the pass rate can only go up. (M)

## What is wrong

The building blocks are good and nothing joins them. `tools/tpdf` has a
`check` subcommand documented as "the thing a corpus runner invokes".
`tools/pdfcmp` is the comparator and now gates on the same metric Tinker uses.
`tools/oracle-diff` drives mutool and pdftoppm as subprocess oracles.
`corpus/` contains a README and nothing else.

There is no runner, no lock file, no ratchet, and no report.

## Scope

- A lock file: each corpus by URL, checksum and licence, so a fetch is
  reproducible and the licence table plan 14 wants has somewhere to live.
- `cargo xtask corpus-fetch` — verifies checksums, never commits the corpora.
- `cargo xtask corpus-run` — one child process per file so a hang or an abort
  in one cannot take the run down, with a timeout.
- A machine-readable report: per file, whether it opened, how many pages,
  whether each rendered, what warnings, how long.
- A committed `ratchet.json`: the pass rate per corpus. CI compares and fails
  on a regression.
- A capability hit-rate table — how many files need JBIG2, JPX, mesh shadings
  — which is the evidence ruling 3 asks for.

## Non-goals

- **Committing the corpora.** Plan 14 is explicit: fetched by pinned checksum,
  never committed. Licences differ per corpus and several forbid
  redistribution.
- **Comparing against an oracle in the same job.** Opening and rendering
  without crashing is one question; matching mutool is another, and
  `oracle-diff` already exists for it. Keep them separate so a regression in
  one is legible.

## Design

**Define `rendered` as "returned a bitmap without crashing or timing out".**
Not "rendered cleanly". Ruling 2 says the engine degrades rather than fails —
so a JBIG2 placeholder is *correct behaviour*, and counting it as a failure
would make every honest degradation a regression and push the ratchet in the
wrong direction. Track degradation on its own axis: a second number, the share
of files that rendered with warnings, which should also trend down but for
different reasons.

**The ratchet is integer arithmetic on counts**, cross-multiplied rather than
compared as floats — `passed_now * total_before >= passed_before * total_now`
— so a corpus whose file count changes does not silently reset the bar.

**No silent caps.** If the runner samples, or stops after N failures, it says
so in the report. A truncated run that reads like a complete one is how a
corpus stops meaning anything.

**What can be built offline:** the lock format and its parser, `corpus-run`
with its child-process isolation and timeouts, the report format, the ratchet
comparator, and every test for all of it — against `testdata/`, against
`DocumentBuilder` output, and against a synthetic corpus of deliberately
malformed files. Only the fetch itself needs network, and therefore so do the
first real `ratchet.json` and the hit-rate table, which cannot be invented.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Lock format, parser, licence table | A malformed lock is rejected with a useful message; the table lists every corpus's licence | S |
| 2 | `corpus-run`, child-process isolation, timeouts | A synthetic corpus containing a file that aborts and one that hangs produces a complete report with both marked | M |
| 3 | Report format and the ratchet comparator | A regression fails; an improvement passes and prints the new numbers to paste; a corpus that grew does not reset the bar | S |
| 4 | `corpus-fetch` with checksum verification | A tampered archive is refused | S |
| 5 | The first real run; `ratchet.json` and the hit-rate table committed | Numbers exist. **Needs network** | S |
| 6 | CI job | Runs on a schedule, not per PR; a regression is a failure | S |

## Dependencies

**Needs first:** nothing for milestones 1–4.

**Unblocks:** [10](10-mesh-shadings.md), [17](17-jbig2-generic-region.md) and
[18](18-jpx-decision.md), all of which ruling 3 says should be scheduled by
this report rather than by judgement. Also every claim about how the engine
handles real documents.

## Risks

| Risk | Mitigation |
| --- | --- |
| Defining `rendered` as "cleanly" would make ruling 2 count against the pass rate | Stated above as a design decision, with degradation on its own axis |
| A corpus changes upstream and the ratchet moves for reasons unrelated to the engine | Pinned by checksum; a checksum change is a deliberate act with its own commit |
| One pathological file hangs the whole run | Child process per file, with a timeout, and both states in the report |
| The run is truncated and reads as complete | The report states what was skipped; no silent caps |
