# Roadmap

The goal: **every capability a mature, widely deployed PDF engine offers,
every capability the rest of the field offers, and more — and every document
a real producer emits opens, renders the way the world renders it,
round-trips, signs, conforms and is searchable, on every target,
deterministically, with no unsafe code, no third-party logic and no silent
failure, at a speed with a number behind it.** The feature docs describe what
the engine does; the roadmap lists what it does not do yet, and nothing else.
Corpus evidence orders the work inside a tier (ruling 3): the row a real
document needs goes before the row none has asked for yet.

Nothing in the roadmap names another engine, viewer, validator or document
tool, the corpora included, each of which is named by its role. A capability
is described on its own terms. `cargo xtask names`, inside `cargo xtask
check`, fails on a name its hash list holds ([verification.md](verification.md)).

## Bugs

Known defects, each with the test that shows it, listed here until a step
fixes it. A case a row's work cannot fix by the owner's rules is listed here,
and its row is narrowed to it.

- **Page one of a linearized file, opened from its head, with
  `bundled-fonts`.** `crates/tinker-pdf/tests/streaming_open.rs`'s test
  that renders page one of each linearized file of the file-structure
  torture corpus from the file's head fails with `bundled-fonts` where
  that corpus is fetched: some files draw a page one other than the
  buffered open draws. The default build draws the same page, and CI,
  which fetches no corpus, skips it.
- **A line of a right-to-left paragraph read alone.** The reader puts each
  line in logical order by itself, so a line of an Arabic paragraph that
  opens with `25٪`, on a page that does not state its text, reads
  `25 ٪يوم`, where its paragraph would keep the sign with its number. FT-37
  resolves a paragraph across its lines.

## Where the steps are

One file per tier, under [roadmap/](roadmap/), named by its rows' prefix.
A step's ID is its row's and a number: OC-01's first step is `OC-01.1`, and
`grep -n '^| OC-01.1 |' docs/roadmap/OC.md` finds its row and no other row
that depends on it.

| Prefix | Tier | File |
| --- | --- | --- |
| MS | Tier 0 — measure what is not measured | [roadmap/MS.md](roadmap/MS.md) |
| K | Tier 1 — prove correctness | [roadmap/K.md](roadmap/K.md) |
| P | Tier P — performance | [roadmap/P.md](roadmap/P.md) |
| RC | Rendering and colour | [roadmap/RC.md](roadmap/RC.md) |
| FT | Fonts and text | [roadmap/FT.md](roadmap/FT.md) |
| ED | Editing, document operations and creation | [roadmap/ED.md](roadmap/ED.md) |
| AN | Annotations, forms and XFA | [roadmap/AN.md](roadmap/AN.md) |
| FJ | Form JavaScript | [roadmap/FJ.md](roadmap/FJ.md) |
| SG | Signatures, with private-key signing in the engine | [roadmap/SG.md](roadmap/SG.md) |
| SE | Security | [roadmap/SE.md](roadmap/SE.md) |
| FI | Formats in | [roadmap/FI.md](roadmap/FI.md) |
| FO | Formats out and print production | [roadmap/FO.md](roadmap/FO.md) |
| SD | Standards | [roadmap/SD.md](roadmap/SD.md) |
| SU | Surface: CLI, bindings, packaging, API stability, documentation | [roadmap/SU.md](roadmap/SU.md) |
| CD | Container depth | [roadmap/CD.md](roadmap/CD.md) |
| OC | OCR, the last implementation tier | [roadmap/OC.md](roadmap/OC.md) |
| H | What only the owner can do: decisions, access, purchases, releases | [roadmap/H.md](roadmap/H.md) |
| NG | Named non-goals: the hard limits | [roadmap/NG.md](roadmap/NG.md) |

## Rows and steps

A **row** is a capability, a gap or a defect: its title and what it is, its
**exit criterion** (the first-party test or local proof that shows it done), its
design **Doc** where it has one, and what it **Depends on**. Its **steps**
are its work, each one change with its tests, in a table: the step, its
kind, what it does, where it lands, its exit test, and what it depends on.

| Kind | What the step lands | Its exit is |
| --- | --- | --- |
| `design` | a section of a design doc in [design/](design/): scope, non-goals, design, milestones, risks | the section, naming each later step's exit test |
| `build` | new capability: code and its tests | named tests |
| `fix` | corrected behaviour | the test that shows the defect, now passing |
| `test` | conformance data, fixtures, a harness or a suite, no engine change | the suite runs and prints its `RAN` banner where it can skip |
| `measure` | a census or benchmark whose committed output is a number | the committed output and the test or local run that regenerates it |
| `ci` | a local proof's tooling and its run (an `xtask` task or a command [verification.md](verification.md) gives), or a step of the release workflow | the tooling exists and its run is green: a local run recorded in the row, or `release.yml`'s run on `main` |
| `doc` | documentation only | the doc, and any example compiles and runs |

A step can start once everything its **Depends on** names is done: the steps
it names have landed, the rows it names have left the roadmap, and the H rows
it names are decided. A decision only the owner can take is an H row, never
a choice inside a step; so is anything a change would cost that works today.

## How the roadmap changes

- A step leaves its step file in the commit that lands it, and its ID is
  never reused. A row leaves when it has no steps left, with its feature
  doc's refusal row.
- A step that turns out bigger than one change is split in place, its pieces
  numbered `<step>a`, `<step>b`, …
- New work enters as a row with its steps, in its tier's file.
- A row whose exit can never go green, for a reason outside this
  repository's power, moves to [roadmap/NG.md](roadmap/NG.md) with that
  reason.
- An exit criterion is a first-party test or a local proof (ruling 13). CI
  runs the unit and integration tests on Linux and Windows on a pull request
  and before `release.yml` releases on the owner's merge to `main`; every
  check — the tests an exit says CI runs, fuzzing, the corpora, the benches,
  peak memory, the bindings' parity, the wasm footprint and fingerprints,
  licences and the oracle suites — is run locally by whoever makes the
  change (ruling 15), the Linux leg in a container, as
  [verification.md](verification.md) gives it, and recorded in the row it
  closes. An outside program may host the code, fetch bytes for it or
  generate a fixture once, and never says whether an output is right.
- A new capability brings its speed row into the benchmark set (MS-04) and
  its determinism fingerprint, entered in the determinism roster
  (`xtask/src/determinism.rs`) in the commit that lands it.

## Order of work

The owner's list below is the order: take the first step in it whose
**Depends on** is done, several lanes at once, each in its own worktree. A
step that waits on the owner is passed over until the owner decides, and a
step leaves the list when it leaves its file. MS-01 closes once the P rows
that land its guards have closed.

**Long proofs and wall-clock timing run after the list** (ruling 15). A step
whose exit is a run or census over the fetched corpora, a fuzz campaign, a
peak checked over several runs, or a wall-clock figure, band, ratio or floor
belongs to the end phase, and while the list runs a dependency on such a step
is met for every step outside the phase. A step this order split has an `a`
piece in the list, which lands its code (a bench operation unmeasured), and a
`b` piece in the end phase.

**The list.**

1–100: MS-25.2, MS-02.4b,
P-02.1, P-02.2, P-02.3, P-02.4, P-02.5, P-02.6, P-02.7, P-03.2,
P-04.3, P-04.4, P-04.5, P-04.7, P-05.5, P-05.8, P-28.1, MS-09.2a, MS-09.3a,
MS-10.1a, MS-10.2, MS-12.3, K-03.2, K-03.3, K-03.4, K-03.5, K-03.6, K-03.7,
K-03.8, K-03.8b, K-04.1, K-04.2, K-04.3, K-04.4, K-04.5, K-04.6, K-04.7, K-04.8,
K-04.9, K-04.10, K-04.11a, K-05.1, K-05.2, K-05.3, K-05.4, K-05.5, K-05.6b,
K-05.7, K-05.8, K-07.2, K-07.3, FT-24.1, FT-24.3, K-08.1, K-08.2, K-08.3,
K-08.4, OC-01.1, OC-01.2, OC-01.3, K-08.6, K-09.1, K-09.2, K-10.1, K-10.2,
K-10.3, K-16.1, K-18.1, K-19.1, P-06.1, P-06.2, P-06.3, P-07.1, P-07.2,
P-07.3, P-07.4, P-07.5, P-07.6, P-07.7, P-07.8, P-07.9a, P-08.1, P-08.2,
P-08.3, P-08.4, P-08.5, P-08.6, P-08.7, P-08.8, P-09.1, P-09.2, P-09.3,
P-09.4, P-09.5, P-09.6.

101–200: P-09.7, P-09.8, P-09.9a, P-15.1, P-10.1, P-10.2, P-10.3, P-10.4a,
P-10.5, P-10.6, P-10.7, P-10.8, P-10.9, P-15.2, P-15.3, P-15.4, P-15.5,
P-15.6, P-15.7, P-15.8, P-15.9, P-15.10a, P-10.10, P-10.11, P-10.12a,
P-08.9, P-11.1, P-11.2, P-11.3, P-11.4, P-12.1, P-12.2, P-12.3, P-12.4,
SG-01.1, P-13.1, P-13.2, P-13.3, P-13.4, P-13.5, P-13.6, P-13.7, SG-01.2,
SG-01.3, SG-01.4, SG-04.2, SG-01.5, SG-01.6, P-13.8, P-14.1, P-14.2, P-14.3,
P-14.4, P-14.5, P-16.1, P-16.2, P-16.3, P-16.4, P-16.5, P-16.6, P-16.7,
P-16.8, P-16.9, P-16.10a, P-17.1, P-17.2, P-17.3, P-17.4, P-17.5, P-17.6,
P-17.7, P-17.8a, P-18.1, P-18.2, P-18.3, P-18.4, P-18.5, P-19.1, P-20.1a,
P-20.2, P-21.1, P-22.1, P-22.2, P-22.3, RC-01.1, RC-01.2, P-22.4, P-22.5,
P-22.6, P-22.7, P-23.1, P-23.2, P-24.1, P-24.2, P-24.3, P-24.4, P-25.1,
P-25.2, P-25.3, P-25.4.

201–300: P-26.1, P-26.2, P-26.3, P-26.4, RC-01.3, RC-01.4, RC-01.5, RC-01.6,
RC-01.7, RC-01.8a, RC-03.1, RC-04.1, RC-04.2, RC-04.3, RC-04.4, RC-04.5,
RC-04.6, RC-04.7, RC-04.8, RC-04.9, RC-04.10, RC-03.3, RC-03.4, RC-04.11,
RC-03.2, RC-03.5, RC-04.12, RC-04.13, RC-04.14, RC-03.6, RC-03.7, RC-03.8,
RC-03.9a, RC-04.15, RC-04.16, RC-04.17a, RC-05.1, RC-20.1, RC-20.2, RC-20.3,
RC-20.4, RC-20.5, RC-20.6, RC-20.7, RC-20.8, RC-20.9, RC-20.10, RC-20.11,
RC-20.12, RC-20.13, RC-20.14, RC-05.2, RC-05.3, RC-05.4, RC-05.5, RC-05.6a,
RC-06.1, RC-06.2, RC-06.3, RC-06.4, RC-06.5, RC-06.6, RC-06.7a, RC-07.1,
RC-07.2, RC-07.3, RC-07.4, RC-07.5, RC-07.6, RC-07.7, RC-07.8, RC-07.9,
RC-07.10, RC-07.11, RC-07.12, RC-07.13, RC-07.14, RC-07.15, RC-07.16a,
RC-08.1, RC-08.2, RC-08.3, RC-08.4, RC-08.5, RC-08.6, RC-08.7, RC-08.8,
RC-08.9, RC-08.10, RC-08.11, RC-08.12a, RC-09.1, RC-09.2, RC-09.3, RC-09.4,
RC-09.5, RC-09.6, RC-09.7a, RC-10.1, RC-10.2.

301–400: RC-10.3, RC-10.4, RC-10.5, RC-10.6, RC-10.7a, RC-11.1, RC-11.2,
RC-11.3, RC-11.4, RC-11.5, RC-11.6, RC-11.7a, RC-12.1, RC-12.2, RC-12.3,
RC-12.4, RC-12.5, RC-12.6, RC-12.7a, RC-13.1, RC-13.2, RC-13.3, RC-13.4,
RC-13.5, RC-14.1, RC-14.2, RC-14.3, RC-14.4, RC-14.5, RC-15.1, RC-15.2,
RC-15.3, RC-15.4, RC-15.5a, FI-05.1, FO-01.1, RC-16.1, RC-16.2, RC-16.3,
RC-16.4, RC-16.5, RC-17.1, RC-17.2, RC-17.3, RC-17.4, RC-17.5, RC-17.6a,
SU-12.1, SU-12.2, RC-18.1, RC-18.2, RC-18.3, RC-18.4, RC-18.5, RC-18.6,
RC-18.7, RC-18.8, RC-18.9, RC-18.10a, RC-19.1, RC-19.2, RC-19.3, RC-19.4,
RC-19.5, RC-20.15a, RC-21.1, RC-21.2, RC-21.3, FT-01.1, FT-01.2, FT-01.3,
FT-01.4, FT-01.5, FT-01.6, FT-01.8, FT-01.9, FT-01.10, FT-01.11, FT-02.1,
FT-02.2, FT-02.3, FT-02.4, FT-02.6, FT-02.8, FT-15.1, FT-15.2, FT-15.3,
FT-15.4, FT-15.5, FT-15.6, FT-15.7, FT-02.5, FT-02.7, FT-02.9, FT-03.1,
FT-03.2, FT-03.3, FT-03.4, FT-04b.1, FT-04a.1.

401–500: FT-04a.2, FT-04a.3, FT-04a.4, FT-04a.5, FT-04b.2, FT-04b.3,
FT-04b.4, FT-04b.5, FT-04b.6, FT-04b.7, FT-04b.8, FT-04b.9, FT-04b.10,
FT-04b.11, FT-04b.12, FT-04b.13, FT-04b.14, FT-04b.15, FT-04b.16, FT-04c.1,
FT-04c.2, FT-04c.3, FT-04c.4, FT-04c.5, FT-04c.6, FT-04c.7, FT-04c.8,
FT-04c.9a, FT-04d.1, FT-04d.2, FT-04d.3, FT-04d.4, FT-04d.5, FT-04d.6,
FT-04d.7, FT-04d.8a, FT-05.1, FT-05.2, FT-05.3, FT-05.4, FT-05.5, FT-05.6,
FT-05.7, FT-05.8, FT-05.9, FT-05.10, FT-05.11, FT-05.12, FT-05.13, FT-05.14,
FT-05.15, FT-05.16, FT-05.17a, FT-06.1, FT-06.2, FT-06.3, FT-06.4, FT-06.5,
FT-06.6, FT-06.7, FT-06.8, FT-06.9, FT-06.10, FT-06.11, FT-06.12, FT-06.13,
FT-06.14, FT-06.15, FT-06.16, FT-06.17a, FT-06.20, FT-07a.1, FT-07a.2,
FT-07a.3, FT-07a.4, FT-07a.5, FT-07a.6, FT-07a.7a, FT-07b.1, FT-07b.2,
FT-07b.3, FT-07b.4, FT-07b.5, FT-07b.6, FT-07b.8, FT-07b.9a, FT-07c.1,
FT-07c.2, FT-07c.3, FT-07c.4, FT-07c.5, FT-07c.6, FT-07c.7, FT-07c.8,
FT-07c.9, FT-07c.10, FT-07c.11, FT-07c.12, FT-07c.13, FT-07c.14.

501–600: FT-07c.15, FT-07c.16, FT-07c.17, FT-07c.18, FT-07c.19, FT-07d.1,
FT-07d.2, FT-07d.3, FT-07d.4, FT-07d.5, FT-07d.6, FT-07d.7, FT-07d.8,
FT-07d.9, FT-07d.10, FT-07d.11, FT-07d.12, FT-07d.14, FT-07d.15, FT-07d.16,
FT-07e.1, FT-07e.2, FT-07e.3, FT-07e.4, FT-07e.5, FT-07e.6, FT-07e.7,
FT-07e.8a, FT-07e.9, FT-07e.11, FT-08.1, FT-08.2, FT-08.3, FT-08.4, FT-08.5,
FT-08.6, FT-08.7, FT-08.8, FT-08.9, FT-08.10, FT-08.11, FT-08.12, FT-08.13,
FT-08.14, FT-08.15, FT-08.16, FT-08.17, FT-08.18a, FT-08.19, FT-09.1,
FT-09.2, FT-10.1, FT-10.2, FT-10.3, FT-10.4, FT-10.5, FT-10.6, FT-11.1,
FT-11.2, FT-11.3, FT-11.4, FT-11.5, FT-11.6, FT-11.7, FT-11.8, FT-11.9,
FT-11.11, FT-12.1, FT-12.2, FT-12.3, FT-12.4, FT-12.5, FT-12.6, FT-12.7,
FT-13.1, FT-13.2, FT-13.3, FT-13.4, FT-13.5, FT-13.6, FT-13.7, FT-14.1,
FT-14.2, FT-14.3, FT-14.4, FT-14.5a, FT-14.6, FT-14.7, FT-15.8, FT-16.1,
FT-16.2, FT-16.3, FT-16.4, FT-16.5, FT-16.6, FT-17.1, FT-17.2, FT-17.3,
FT-17.4, FT-17.5.

601–700: FT-17.6, FT-17.7, FT-17.8, FT-17.9, FT-17.10, FT-17.11, FT-17.12,
FT-17.13, FT-18.1, FT-18.2, FT-18.3, FT-18.4, FT-18.5, FT-18.6, FT-18.7,
FT-18.8, FT-18.9, FT-18.10, FT-18.11, FT-18.12, FT-19.1, FT-19.2, FT-19.3,
FT-19.4, FT-19.5, FT-19.6, FT-19.8, FT-20.1, FT-20.2, FT-20.3, FT-20.4,
FT-20.5, FT-20.6, FT-21.1, FT-21.2, FT-21.3, FT-21.4, FT-21.5, FT-21.6,
FT-22.1, FT-22.2, FT-22.3, FT-22.4, FT-22.5, FT-22.6, FT-23.1, FT-23.2,
FT-23.3, FT-23.4, FT-24.2, FT-24.4, FT-24.5, FT-24.6, FT-25.1, FT-25.2,
FT-25.3, FT-30.1, FT-30.2, FT-30.3, FT-30.4, FT-30.5, FT-30.6, FT-25.4,
FT-25.5, FT-25.6, FT-25.7, FT-25.8, FT-25.9, FT-25.10, FT-25.11, FT-26.1,
FT-26.2, FT-26.3, FT-26.4, FT-27.1, FT-27.2, FT-27.3, FT-27.4, FT-28.1,
FT-28.2, FT-28.3, FT-28.4, FT-28.5, FT-28.6, FT-29.1, FT-29.2, FT-29.3,
FT-29.4, FT-29.5, FT-29.6, FT-29.7, FT-29.8, FT-29.9, FT-29.10, FT-32.1,
FT-32.2, FT-32.3, FT-32.4, FT-32.5, FT-32.6.

701–800: FT-32.7a, FT-33.1, FT-33.2, FT-33.3, CD-04.1, CD-04.2, CD-04.3,
CD-04.4, CD-04.5, CD-04.6, CD-04.7, FT-34.1, SD-19.1, SD-19.4, CD-04.8,
FT-34.2, FT-34.3, FT-34.4, FT-34.5, FT-34.6, FT-34.7, FT-35.1, FT-35.2,
FT-35.3, FT-35.4, FT-35.5, FT-36.1, FT-36.2, FT-37.1, FT-37.2, FT-37.3,
ED-01.1, ED-01.2, ED-01.3, ED-01.4, ED-01.5, ED-01.6, ED-01.7, ED-01.8,
ED-01.9, ED-01.10, ED-01.11, ED-01.12, ED-01.13, ED-01.14, ED-01.15,
ED-01.16, ED-01.17, ED-01.18, ED-01.19, ED-02.1, ED-02.2, ED-02.3, ED-02.4,
ED-02.5, ED-02.6, ED-02.7, ED-02.8, ED-02.9, ED-02.10, ED-02.11, ED-02.12,
ED-02.13, ED-02.14, ED-02.15, ED-02.16a, ED-03.1, ED-03.2, ED-03.3, ED-04.1,
ED-04.2, ED-04.3, ED-04.4, ED-04.6, ED-05.1, ED-05.2, ED-05.3, ED-06a.1,
ED-06a.3, ED-25.1, ED-06a.2, ED-06a.4, ED-07.1, ED-07.2, ED-07.3, ED-07.4,
ED-08.1, ED-08.4, ED-08.5, ED-10.1, ED-08.2, ED-08.3, ED-08.6, ED-09.1,
ED-09.2, ED-09.3, ED-09.4, ED-09.5, ED-10.2, ED-10.3.

801–900: ED-11.1, ED-11.2, ED-12.1, ED-12.2, ED-12.3, ED-13.1, ED-30.1,
ED-30.2, ED-13.2, ED-13.3, ED-13.4, ED-13.5, ED-13.6, ED-13.7, ED-13.8,
ED-15.1, ED-45.1, ED-45.2, ED-45.3, ED-45.4, ED-15.2, ED-15.3, ED-14.1,
ED-14.2, ED-15.4, ED-16.1, ED-16.2, ED-17.1, ED-17.2, ED-17.3, ED-17.4,
ED-17.5, ED-18.1, ED-18.2, ED-19.1, ED-19.2, ED-19.3, ED-20.1, ED-21.1,
ED-21.2, ED-21.3, ED-22.1, ED-22.2, ED-31.1, ED-31.2, ED-31.3, ED-31.4,
ED-22.3, ED-22.4, ED-23.1, ED-23.2, ED-23.3, ED-23.4, ED-23.5, ED-24.1,
ED-24.2, ED-24.3, ED-24.4, ED-24.5, ED-24.6, ED-25.2, ED-25.3, ED-25.4,
ED-26.1, ED-26.2, ED-26.3, ED-26.4, ED-26.5, ED-26.6, ED-26.7, ED-26.8,
ED-26.9, ED-26.10, ED-26.11, ED-26.12, ED-26.13, ED-26.14a, ED-26.15,
ED-27.1, ED-27.2, ED-27.3, ED-27.4, ED-27.5, ED-27.6, ED-27.7, ED-28.1,
ED-28.2, ED-28.3, ED-28.4, ED-29.1, ED-29.2, ED-29.3, ED-29.4, ED-32.1,
ED-32.2, ED-32.3, FI-27.1, ED-33.1, ED-33.2, ED-34.1.

901–1000: ED-34.2, ED-34.3, ED-35.1, ED-35.2, ED-35.3, ED-35.4, AN-02.1,
AN-02.4, AN-06.1, ED-36a.1, ED-36a.2, ED-36b.1, ED-36b.2, ED-36b.3,
ED-36a.3, ED-36c.1, ED-36c.2, ED-36d.1, ED-36d.2, ED-36d.3, ED-36e.1,
ED-36e.2, ED-36e.3, ED-36e.4, ED-36e.5, ED-36f.1, ED-36f.2, ED-37.1,
ED-37.2, ED-37.4, ED-37.5, ED-37.6, ED-37.7, FI-15.1, FO-18.1, FO-18.2,
ED-37.3, ED-37.8, ED-38.1, ED-38.2, ED-38.3, ED-39.1, ED-39.2, ED-39.3,
ED-40.1, ED-40.2, ED-40.3, FI-14.1, FI-14.2, FI-16.1, FI-16.2, ED-41.1,
ED-41.2, ED-41.3, ED-41.4, ED-42.1, ED-42.2, ED-43.1, ED-43.2, ED-43.3,
ED-44.1, ED-44.2, ED-44.3, ED-44.4, ED-44.5, ED-44.6, ED-44.8, ED-46.1,
ED-47.1, ED-48.1, ED-48.2, ED-49.1, ED-51.1, ED-51.2, AN-02.2, AN-02.3,
AN-02.5, AN-03.1, AN-03.2, AN-04.1, AN-04.2, AN-05.1, AN-05.2, AN-06.2,
AN-06.3, AN-06.4, AN-06.5, AN-07.1, AN-07.2, AN-07.3, AN-07.4, AN-09.1,
AN-09.2, AN-09.3, AN-10a.1, AN-10a.2, AN-10b.1, AN-10b.2, AN-10b.3,
AN-10b.4.

**The end phase.** First the full test and gate run on every target, each
failure traced to the step that wrote it and each moved pin reviewed by the
owner; K-03.8c lands from that run, its exact defect lists read off it. Then
the long proofs, several lanes at once:

MS-18.2, K-06.2, K-02.5, MS-13.2, MS-13.3, MS-13.4, MS-13.5, K-04.12,
RC-16.6a, RC-20.16, FT-01.7, FT-01.12, FT-11.10, FT-14.9, FT-30.7, FT-30.9,
FT-31.2, FT-31.3, ED-04.5, ED-25.5, ED-38.4, ED-44.7, AN-01.2, K-04.11b,
MS-22.2b, MS-25.2b, MS-27.2b, K-05.3b, RC-01.9b.

Then the timing runs, on the named machine with nothing else running:

MS-24.1, MS-24.2, MS-04.10, MS-02.6, MS-02.7, P-01.5, P-02.8, P-03.7,
P-04.6, P-05.9, MS-10.3, P-06.4, P-07.10, P-09.11, P-10.13, P-11.5, P-12.5,
P-13.9, P-14.6, P-16.11, P-17.9, P-20.3, P-20.6, P-21.2, P-21.3, P-22.8,
P-23.3, P-24.5, P-25.5, P-26.5, P-04.8, MS-07.4, P-08.10, P-15.11, P-18.6,
MS-09.5; then the `b` pieces, each after its `a`: MS-09.2b, MS-09.3b,
MS-10.1b, MS-10.2b, P-07.9b, P-09.9b, P-10.4b, P-10.12b, P-15.10b, P-16.10b, P-17.8b,
P-20.1b, RC-01.8b, RC-03.9b, RC-04.17b, RC-05.6b, RC-06.7b, RC-07.16b,
RC-08.12b, RC-09.7b, RC-10.7b, RC-11.7b, RC-12.7b, RC-15.5b, RC-16.6b,
RC-17.6b, RC-18.10b, RC-20.15b, FT-04c.9b, FT-04d.8b, FT-05.17b, FT-06.17b,
FT-07a.7b, FT-07b.9b, FT-07e.8b, FT-08.18b, FT-14.5b, FT-32.7b, ED-02.16b,
ED-26.14b.

**After them**, the capability tiers in the order of the table above, then
container depth, then OCR.

**Dependencies that cross tiers.** A row waits for every row it names,
wherever that row sits, and the named row is pulled forward to meet it.
These pull a later tier's row forward: SG-01 → P-13; AN-06 → ED-36d; FJ-01 →
AN-31; FJ-03e → AN-30; FJ-06 → AN-15; FJ-07 → AN-24; FJ-11 → AN-11c; SG-06 →
AN-17; SG-09 → FJ-17; SG-13 → AN-34; SE-07 → SG-25; FI-14 → ED-41; FI-27 →
ED-33; FO-18 → ED-37; SD-02, SD-12 → FO-27; SU-12 → RC-18; SU-15 → SG-28;
CD-01 → AN-29; CD-04 → FT-34; CD-16 → FI-33. These are the long chains that
run forward across tiers: RC-01, RC-02, RC-20 → ED-06b
→ SD-02, SD-12; FO-10 → FO-08b, SD-02 (part 1), SD-12; FT-12 → SD-02, SD-12;
FJ-01 → FJ-02 → FJ-03a…FJ-03f → AN-30, CD-12, SU-11; FJ-18 → FJ-10…FJ-13;
SG-04 → SG-02; SG-01, SG-02, SG-05 → SG-06 → SG-07, SG-08, SG-09; SG-06,
SG-08 → SG-27 → SG-28; SG-11 → SG-10 → SG-12; SG-13 → SG-14, SE-15, AN-34,
CD-13; FI-36 → FI-02d, FI-02f, FI-02g, FO-19; FT-30, FT-31 → SD-07, OC-06,
FO-14a, FT-25; ED-01 → ED-02, ED-03; ED-49 → ED-50 → ED-12, OC-15b, AN-24;
ED-26, ED-27 → OC-12; MS-01 lands with every P row; MS-13 (the corpus runs)
closes AN-01, SG-26, SD-01, SD-04, SD-11, FT-30, FT-31, CD-01 and CD-08.
