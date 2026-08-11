# The wasm determinism job has never run

Ruling 4 says the same document renders to the same bytes on Linux, Windows,
macOS and wasm. Three of those four are proved on every push:
`tests/determinism.rs` hashes four rendered pages against committed
fingerprints and rides the existing matrix. The fourth job is written and has
never executed, because neither the target nor the runtime is installed here.
When this is done, the ruling is demonstrated on all four. (S)

## What is wrong

Nothing is broken. The mechanism is guarded — no pixel path calls the
platform's `libm`, and `cargo xtask libm` fails the build if one starts to.
The property is demonstrated natively.

The `wasm-determinism` job runs the same test on `wasm32-wasip1` under
wasmtime, installing both through the CI action. It has never been executed:
`wasm32-wasip1` is not installed in this environment and neither is wasmtime,
so the job is written from the documentation rather than from a passing run.
It is recorded in STATUS as written-and-unverified rather than counted as
done.

## Scope

- Verify the job actually runs, in CI or locally with the target and runtime
  installed.
- Confirm the fingerprints match the native ones — which is the whole point,
  and is the outcome that cannot be assumed.
- If they differ, find the arithmetic that is not target-stable. **Do not
  update the fingerprints.**
- Extend the fixture set as new pixel paths land — every plan here that moves
  pixels should add one.

## Non-goals

- **`wasm32-unknown-unknown` test execution.** It cannot run a test binary
  without a JavaScript harness. `wasm32-wasip1` gives the same code generation
  with a `main` a runner can execute, which is what the property needs. The
  existing build check on `wasm32-unknown-unknown` stays.

## Design

The interesting case is failure. If wasm disagrees with the native targets,
the fingerprints are not the thing to change — some arithmetic is not
target-stable, and this job is the only thing that would ever say so. The test
file's own documentation states that distinction, and the job's comment
repeats it, because the instinct on a red build is to update the expected
value.

Plausible culprits, in the order worth checking: an `as` cast whose overflow
behaviour differs; a `usize` width assumption; something in the flattening
tolerance; or a float comparison that is not the correctly-rounded operation
it looks like.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | The job runs | A CI run shows it executing rather than skipping | S |
| 2 | It agrees with the native matrix | All four fingerprints identical across four targets. If not, the divergence is found and fixed — not baselined | S |
| 3 | STATUS updated | The row moves out of "Not built" once it has actually run | S |
| 4 | Fixture growth as pixel paths land | Each of [09](09-tiling-patterns.md), [10](10-mesh-shadings.md), [11](11-transparency-groups.md), [12](12-image-sampling.md) adds a fixture | S |

## Dependencies

**Needs first:** nothing. Needs a runner with the target and wasmtime, which
CI has and this environment does not.

**Unblocks:** the last quarter of ruling 4's claim.

## Risks

| Risk | Mitigation |
| --- | --- |
| The job is red on first run and the fingerprints get updated to make it green | Said in three places — the test's module docs, the job's comment, and here. A wasm-versus-native divergence is a bug in the arithmetic |
| The job silently skips and looks like it passed | Milestone 1's exit criterion is that a run shows it *executing*, not that CI is green |
| Fixtures drift out of date as rendering changes, so the job proves less over time | Milestone 4 ties fixture growth to the plans that move pixels |
