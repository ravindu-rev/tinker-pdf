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

*Amended, 19 August 2026.* **Milestone 4 is now done** — all four of those
plans landed their fixtures, and the section below closes it. The section
below also converts the linux leg from claimed to measured.

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

---

## Progress — 19 August 2026, the linux leg and milestone 4

**Two of ruling 4's four targets became three, and milestone 4 closed.** What
is left is macOS, which this machine cannot settle, and a CI run nobody has
watched — and both are named below with what would settle them rather than
left as "still owed".

### Linux, measured

The `As built` above says the linux and macOS legs "come only from the CI
matrix". Linux does not any more. WSL2 is on this host and
`x86_64-unknown-linux-gnu` is a native target inside it, so the suite was built
and run there on **`stable`** — the toolchain `ci.yml`'s three-OS matrix uses,
rather than the nightly the fuzzing route needs:

```text
rustc 1.97.1 (8bab26f4f 2026-07-14)
Linux 6.6.87.2-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC x86_64 GNU/Linux
```

`cargo test -p tinker-pdf --test determinism` passes, and so does
**`cargo test --workspace`: 2 243 passed, 0 failed, 10 ignored**, which is the
Windows count to the test. That second run is worth more than it looks. The
determinism file is four tests; the workspace is the whole engine, and running
it on a second operating system is the first time anything here has done that
outside CI. It was run twice — once at `a6a4fef`, and again at `9d4ffcf` so
that the numbers describe the tree they are committed beside rather than the
tree they were convenient to measure on.

All **fourteen** fingerprints came back byte-identical to the committed table,
which is `x86_64-pc-windows-msvc`. So did the two byte hashes beside them —
gap 29's synthesised comic and gap 30's synthesised fixed document, which pin
object numbering, dictionary key order and stream framing, none of which a
rendered hash can see.

The values are recorded here rather than left implicit in "it passed", because
a hash says only that two runs agreed and not what they agreed *about*. The
page size and the ink count are what say the thing hashed was a picture:

| Fixture | Page | Ink | SHA-256 |
| --- | --- | --- | --- |
| `text` | 200 x 100 | 1 486 | `b0bc9383d116d84d7a104afc67b3d5dc8e727323ba30262f67121a32b89004c2` |
| `curves` | 130 x 100 | 2 363 | `7924b1b282589efa4bbfc39055af40d9f29c9405d0c95381420706b97163968b` |
| `shading` | 120 x 80 | 9 600 | `813a28f7b119418e76ae52f96f69047b5dec5100a26375294e9de41ed9cc90b5` |
| `blend` | 80 x 80 | 3 600 | `759840c7df7bad4fc49a2d94f763e8b5eca6d9edb64f3af1cdfcd635b2512258` |
| `pattern` | 120 x 80 | 3 230 | `18765f39455bc173f00fc6272449402d0c5db445963b5334e3d511a766199af2` |
| `optional` | 120 x 80 | 4 922 | `e0f2bc33f56dcb85beb7a1770f9cb33e22a1a2cdba1cbb4b838be656370035a1` |
| `image` | 160 x 120 | 5 262 | `8cca4e2c1380f630e1c85da93b3a6add4349156d704adbffca7d45d917244f38` |
| `transparency` | 120 x 80 | 8 066 | `c120574918fcfadb0b33f3f9faa4f0c10a10cc760cd9e9830bedf31463e3f059` |
| `tiling` | 120 x 80 | 4 488 | `aa7b2df6bd7613fb53c696ed4b9018a00d1aa4dece2ffe82775c40bfaa1a5011` |
| `jbig2` | 128 x 120 | 1 383 | `cd20bc1e5c786e245402ba94d700f2a91a267c36e0922d2bc98be5e897839abd` |
| `mesh` | 120 x 80 | 9 311 | `546f7f9e61572460b1b76610719e772b69625651d6a6b3b820ab30538be7d693` |
| `jpx` | 160 x 60 | 5 180 | `d9d0a1f733de50ca06fae32655bc240854d573679698ce7a8e8095640972ef4d` |
| `cbz` | 40 x 40 | 1 403 | `9e92c73984cff79feef04dcc984c52f04beda91bf3087a50ce9c17b3fc275aea` |
| `xps` | 612 x 792 | 20 333 | `3e91e30f90903a7b5a91f0442c965acc2519ab22ce7e1c9c5f9b6392e2f74751` |

**The table is printed by a scaffold, not by the suite.** `fingerprint` hashes
and compares; it does not report. The pristine tree was run first and passed,
and then a copy — in WSL, never committed — had one `eprintln!` added to
`fingerprint`'s tail so the values that passed could be written down. That
distinction is the point of recording it: the pass is the measurement, the
table is a transcript of it.

**Nothing had to be changed to make it run**, which is itself the finding
worth having. The list of plausible culprits in this plan's Design section —
an `as` cast whose overflow behaviour differs, a `usize` width assumption, a
flattening tolerance, a float comparison that is not the correctly-rounded
operation it looks like — has now been checked against a third target and
caught nothing three times.

### What three of four is, and is not

| Target | Status | How |
| --- | --- | --- |
| `x86_64-pc-windows-msvc` | **measured** | the committed table is this one |
| `wasm32-wasip1` | **measured** | wasmtime 47.0.3, this host, August 2026 |
| `x86_64-unknown-linux-gnu` | **measured** | WSL2, this host, 19 August 2026 |
| `aarch64-apple-darwin` | **claimed** | `ci.yml`'s `macos-14` leg, never observed |

The pairing that had already been checked — 64-bit Windows against 32-bit
wasm — is the one most likely to catch a width assumption, and linux against
Windows is a weaker test in that respect: both are 64-bit little-endian
x86-64, so the arithmetic is the same instructions. What it *does* cover is
everything below the arithmetic — a different `std`, a different allocator, a
different `libm` (which no pixel path may call, and `cargo xtask libm` is what
enforces that), a different linker and a different set of default codegen
flags. That is a real axis and it is now closed.

**macOS is not achievable on this machine, and no amount of care makes it so.**
There is no Apple hardware here, no macOS virtual machine, and Apple's targets
cannot be cross-*run* from Windows or from Linux — cross-*compiling* to
`aarch64-apple-darwin` needs the SDK, and even with it, executing the test
binary needs Darwin. Recorded plainly rather than approximated: **what would
settle it is one observed CI run** in which `ci.yml`'s `test` job is green on
`macos-14` and the `wasm-determinism` job is green on the same commit. That is
also, and separately, the outstanding half of milestone 1 — the job's exit
criterion is that a *run* shows it executing, and no run of it has been
watched. Both remaining items are the same act: push, and look at the four
jobs.

### Milestone 4, closed

Milestone 4 asked each of [09](09-tiling-patterns.md),
[10](10-mesh-shadings.md), [11](11-transparency-groups.md) and
[12](12-image-sampling.md) to add a fixture as it landed. All four did, and the
`As built` above was written before the last of them:

| Owed by | Fixture | What had no fingerprint before it |
| --- | --- | --- |
| [09](09-tiling-patterns.md) | `tiling` | a rasterised cell, a lattice, `PaintType 2`. Gap 07's `pattern` is a `PatternType 2` shading evaluated per pixel and reaches none of it |
| [10](10-mesh-shadings.md) | `mesh` | the only fixture whose hash depends on a *count* chosen from a device-space measure, so a subdivision step landing one different on another target shows here and nowhere else |
| [11](11-transparency-groups.md) | `transparency` | a `/Group` or an ExtGState `/SMask` — none of the seven before it would have moved if clause 11 had never been written |
| [12](12-image-sampling.md) | `image` | an image. Every fingerprint report made while image sampling was being rewritten was true and meant nothing |

Six more arrived from plans milestone 4 does not name — gap 06's `optional`,
gap 07's `pattern`, gap 17's `jbig2`, gap 18a's `jpx`, gap 29's `cbz` and gap
30's `xps` — each for the same reason stated the same way, which is the habit
this milestone was trying to instil. Fourteen fingerprints and two byte
hashes, against the four this plan opened with.

The mechanism the August amendment above asked for is what makes the habit
safe rather than merely customary: `fingerprint` refuses to hash a page that
painted fewer than a stated number of pixels or that reported
`UnreadableFont`, so a fixture that draws nothing fails on the day it is
added rather than becoming a baseline. Every one of the fourteen carries a
floor, and the two whose floors are weaker than the rest — `jpx`, because
both of its failure modes paint, and `cbz`, because ruling 2's placeholder is
ink on every pixel — say so where they are declared.

### Amended, 22 August 2026: fifteen, and a question this plan did not have

The count above was fourteen when it was written. Gap 31's milestone 13 added
the fifteenth — `epub`, a real producer's book — and with it something no
earlier fingerprint could ask.

**Every fingerprint before it hashed a document whose page count was a property
of the file.** A reflowable book's is not: `OpenOptions` states a page box and
the pagination follows it, which gap 31's milestone 4 recorded as the reason
`Document::open_with` exists at all. So determinism for this format is two
claims rather than one — *stable at a given box*, and *different at a different
box* — and `a_book_is_stable_at_each_page_box_and_the_two_boxes_differ` holds
both. A build that ignored the caller's box entirely would satisfy "stable"
twice over, which is the shape this repository has spent a run learning to
distrust.

That test also found the half of it that is easy to miss: a build ignoring the
caller's *width* still produces two different documents, because the height
still differs. Two independent consequences, and the injection matrix reported
it as surviving until both were asserted.

All fifteen reproduce on `wasm32-wasip1` under wasmtime, and on
`x86_64-unknown-linux-gnu` under WSL2, against native Windows.

### What this plan still leaves open

- **macOS.** Claimed, from `ci.yml`, and the claim is now checkable in one
  respect: the `test` matrix names `macos-14` and runs `cargo test --workspace`,
  which is what carries `tests/determinism.rs`. So the job is configured to
  prove ruling 4 there; what is missing is a run somebody watched. **This
  machine cannot supply one** — there is no Apple hardware here, and a Darwin
  target cannot be cross-*run*, only cross-compiled.
- **A CI run of `wasm-determinism`.** The job is written, guarded against
  passing without executing, and has been run by hand on this machine; nobody
  has watched it run *there*. The same act settles both.

**Three of ruling 4's four targets are measured; the fourth is configured and
unobserved.** That is the whole of the difference, and it is a difference no
amount of work on this machine can close — which is why it is written here as a
standing item rather than as a milestone somebody could mistake for undone
work.
