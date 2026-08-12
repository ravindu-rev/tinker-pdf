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

*Amended, August 2026.* One of those four pages was not a page. The `text`
fixture named Helvetica, embedded no font program and drew nothing, because
the engine bundles no faces — its fingerprint was the hash of a blank
200x100 canvas, and the three native targets agreeing on it proved only that
they can all produce white. Had the wasm leg run, it would have agreed too,
and that agreement would have been read as evidence about glyph
rasterisation, which is the densest arithmetic on the pixel path and the
likeliest place for a target to diverge. The fixture now embeds a synthetic
TrueType face of curves and diagonals; the `text` hash changed in the same
commit. Milestone 4's fixture growth inherits the lesson as a mechanism
rather than a warning: `fingerprint` refuses to hash a page that painted
fewer than a stated number of pixels, or that reported `UnreadableFont`, so a
new fixture that draws nothing fails on the day it is added.

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

*Amended, August 2026, by [07](07-stroked-patterns.md).* A fifth fixture,
`pattern`, landed ahead of the four plans named in milestone 4 and is not owed
by any of them. It covers `fill_with_pattern`, which had no fingerprint at
all — the `shading` fixture goes through `sh`, a different loop with a
different inverse transform — and gap 07 is what made a stroked outline reach
it. It reproduces on `wasm32-wasip1`. The four above are still owed.

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

## As built

*August 2026.* Milestones 1 to 3 are done. **Milestone 4 is not**, and it is
not this plan's to do: it belongs to [09](09-tiling-patterns.md),
[10](10-mesh-shadings.md), [11](11-transparency-groups.md) and
[12](12-image-sampling.md), each of which adds its own fixture as it lands.

**The job runs, and it agrees.** The target and wasmtime 47.0.3 are installed
on the development host now, so the job that had only ever been written was
executed:

```bash
CARGO_TARGET_WASM32_WASIP1_RUNNER=wasmtime \
  cargo test -p tinker-pdf --test determinism --target wasm32-wasip1
```

Both tests pass. All four fingerprints came back byte-identical to the
committed table, which is `x86_64-pc-windows-msvc`:

```text
text     98c3e73c83e08654f2d6076aefbe0786be1dd73f3013ca6c9f52fe5d5ed494ee
curves   7924b1b282589efa4bbfc39055af40d9f29c9405d0c95381420706b97163968b
shading  813a28f7b119418e76ae52f96f69047b5dec5100a26375294e9de41ed9cc90b5
blend    759840c7df7bad4fc49a2d94f763e8b5eca6d9edb64f3af1cdfcd635b2512258
```

So did everything the hash is computed from: the same page dimensions, and
the same ink counts — 1486, 2363, 9600 and 3600 pixels — on both. The ink is
worth recording next to the hashes rather than leaving implicit in them,
because a hash says only that two runs agreed and not what they agreed
about. These agreed about 1486 pixels of glyph coverage, not about a blank
page, which is the distinction the August amendment above exists to keep.

**What this does and does not prove.** Two of ruling 4's four targets, on one
host: 64-bit Windows against 32-bit wasm. It is the pairing most likely to
have caught something — a `usize` that is 64 bits on one target and 32 on the
other is the plan's own first suspect and the hardest to notice by reading —
and it caught nothing, which is the good outcome. But the Linux and macOS legs
still come only from the CI matrix, and **no CI run of this job has been
observed**. Four-target agreement needs a green `wasm-determinism` job beside
a green three-OS `test` matrix on the same commit. Three-of-four plus a
locally verified pair is what is actually in hand.

**Nothing had to be fixed to make it build.** Neither obstacle the plan
anticipated appeared. `tinker-pdf` has no external dev-dependencies — the only
thing the test reaches for beyond the facade is `tinker-pdf-crypto`'s SHA-256,
which is an ordinary dependency and already builds for wasm — so there was no
proptest or criterion to fail on the target. And all four fixtures construct
their document bytes in memory rather than reading `testdata/`, so nothing
wants a filesystem WASI does not offer. It compiles for the target under
`RUSTFLAGS=-D warnings` with no warnings.

**The job was changed, for the risk table's second row rather than its
first.** `cargo test` exits 0 when it runs no tests at all — verified, not
assumed: a filter matching nothing prints `0 passed; 0 failed; 2 filtered out`
and returns success. So a `cfg` that excluded these two tests from wasm, or an
`#[ignore]` added on an afternoon when the target was inconvenient, would have
produced a green tick and no rendering. The step now tees its output and greps
it for a non-zero pass count and for `rendering_is_stable_across_targets`
by name, with `pipefail` so that `tee` cannot mask a real failure. Milestone
1's exit criterion is that the job *executes*; this is what makes the green
tick mean that.

**One thing the plan did not say: wasmtime has to be findable by cargo, not
by you.** `CARGO_TARGET_WASM32_WASIP1_RUNNER: wasmtime` resolves through the
`PATH` of the process cargo spawns. In CI `taiki-e/install-action@wasmtime`
puts it there and the bare name is right. Locally, an installer that updated
the user `PATH` may not have reached an already-running shell, and the symptom
is not "wasmtime not found" but cargo trying to execute a `.wasm` file
directly. The absolute path works and is what the runs above used;
`CONTRIBUTING.md` now carries the recipe.
