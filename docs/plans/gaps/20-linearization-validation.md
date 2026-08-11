# Nothing has ever checked the hint tables

The linearized writer emits a complete Annex F file and every offset it
declares is checked against the actual bytes — by tests this engine wrote,
reading output this engine produced. `qpdf --check` and
`qpdf --show-linearization` are the arbiters plan 09 names, and neither has
run. The layout is well tested; the **hint tables are unproven**. When this is
done, an external validator agrees. (S)

## What is wrong

The tests assert the things a reader of our own output can assert: the
parameter dictionary comes first, `/L` is the file length, `/H` points at a
stream, `/E` precedes `/T`, the final `startxref` points into the front of the
file, page one's content is inside `/E` and the last page's is not.

None of that can catch a wrong hint table. The page-offset and shared-object
tables are bit-packed structures whose fields are read by nobody in this
repository — the writer emits them and nothing consumes them. They could be
entirely wrong and every test would still pass.

They are also the part most likely to be wrong. Hint tables are the
least-exercised structure in the format; most viewers ignore them, so bugs
survive for years in real implementations.

## Scope

- Run `qpdf --check` over linearized output.
- Run `qpdf --show-linearization` and compare its reading of the hint tables
  against what the writer intended.
- Wire both into CI as a subprocess oracle (ruling 9), skipped with a clear
  message when qpdf is absent rather than silently passing.
- A round-trip reader for our own hint tables, so the structure is verifiable
  offline too.

## Non-goals

- **Redistributing qpdf.** Ruling 9: oracles are subprocesses, invoked, never
  vendored, outputs used for comparison and never shipped.
- **Matching qpdf's layout.** Two linearizers may lay a file out differently
  and both be conformant. The bar is that qpdf reads ours without complaint,
  not that it would have produced it.

## Design

**The offline half matters as much as the qpdf half**, and can be built now.
A reader for our own bit-packed tables, in the test module, that parses them
back and checks the values against the layout the writer computed: least
objects per page, per-page deltas, the shared-object counts. That is a genuine
check — it catches a field written at the wrong bit width, a header item in
the wrong order, a delta computed against the wrong base — and it needs no
external tool.

What it cannot catch is a misreading of the specification: if the writer and
the round-trip reader share a wrong idea of the field order, they agree. That
is exactly what qpdf is for, and why both halves are wanted.

**Skipped, not silently passed.** A CI job that quietly succeeds when qpdf is
missing is a job that will one day be missing on every runner. It reports
skipped, visibly.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Round-trip reader for the hint tables | Every header field and per-page entry parses back to what the writer computed, for a one-page, a six-page and a shared-resource fixture | S |
| 2 | `qpdf --check` in CI over linearized output | Green on the fixtures; the job reports skipped, not passed, when qpdf is absent | S |
| 3 | `qpdf --show-linearization` compared | qpdf's page count, first-page object and hint-table summary agree with the writer's intent | S |
| 4 | Encrypted linearized output validated too | The same two checks pass on the output of [19](19-encrypt-and-linearize.md) | S |

## Dependencies

**Needs first:** nothing for milestones 1–3. Milestone 4 needs
[19](19-encrypt-and-linearize.md).

**Unblocks:** the claim that linearization works. Until this lands, STATUS
should keep saying the hint tables are unproven — which it does.

## Risks

| Risk | Mitigation |
| --- | --- |
| qpdf is not installed in this environment, so milestones 2–4 cannot be verified locally | Milestone 1 is the offline half and is verifiable now; the CI job is written and marked unverified until a runner has qpdf, exactly as the wasm determinism job is |
| A skipped job looks like a passing job | The job reports its skip explicitly, and STATUS carries the state until it has actually run |
| The round-trip reader and the writer share a misreading | Stated above as the known limit of the offline half; it is why the qpdf half is not optional |
