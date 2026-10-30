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
is described on its own terms.

## Bugs

Known defects, each with the test that shows it, listed here until a step
fixes it. A case a row's work cannot fix by the owner's rules is listed here,
and its row is narrowed to it.

- **Page one of a linearized file, opened from its head, with
  `bundled-fonts`.** `crates/tinker-pdf/tests/streaming_open.rs`'s
  `linearized_files_from_the_qpdf_corpus_render_page_one_from_their_heads`
  fails with `bundled-fonts` where the qpdf corpus is fetched: some files
  draw a page one other than the buffered open draws. The default build
  draws the same page, and CI's `test` job, which has no corpus, skips it.
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
**exit criterion** (the first-party test or CI job that shows it done), its
design **Doc** where it has one, and what it **Depends on**. Its **steps**
are its work, each one change with its tests, in a table: the step, its
kind, what it does, where it lands, its exit test, and what it depends on.

| Kind | What the step lands | Its exit is |
| --- | --- | --- |
| `design` | a section of a design doc in [design/](design/): scope, non-goals, design, milestones, risks | the section, naming each later step's exit test |
| `build` | new capability: code and its tests | named tests |
| `fix` | corrected behaviour | the test that shows the defect, now passing |
| `test` | conformance data, fixtures, a harness or a suite, no engine change | the suite runs and prints its `RAN` banner where it can skip |
| `measure` | a census or benchmark whose committed output is a number | the committed output and the test or job that regenerates it |
| `ci` | a workflow under `.github/workflows/` or an `xtask` job | the job exists and its run is green |
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
- An exit criterion is a first-party test or CI job (ruling 13): an outside
  program may host the code, fetch bytes for it or generate a fixture once,
  and never says whether an output is right.
- A new capability brings its speed row into the benchmark set (MS-04) and
  its determinism fingerprint (K-07).

## Order of work

Take the first step in this list whose **Depends on** is done; a step that
waits on the owner is passed over until the owner decides.

1. **The MS tier** from where it stands: MS-12.2;
   MS-03.1 and MS-02.2; K-02.3, then MS-01.3 to MS-01.7; P-05.3, then
   MS-08.2; MS-07.3. Then MS-14 to MS-21, MS-01 to MS-01.8,
   MS-04, MS-05, MS-06 and MS-02, in that order. MS-01.9 closes MS-01 once
   the P rows that land its guards have.
2. **K-06** (the `jbig2` fuzz target), **K-01** (the external graphics
   state) and **K-02** (default colour spaces).
3. **P-01 to P-05** and **P-28**.

Then the capability tiers in the order of the table above, with signatures'
SG-01…SG-09 and form JavaScript's FJ-01…FJ-04 started as soon as their
owner decisions (H-32…H-36) are in; then container depth; OCR last.

**Dependencies that cross tiers.** A row waits for every row it names,
wherever that row sits, and the named row is pulled forward to meet it.
These pull a later tier's row forward: SG-01 → P-13; AN-06 → ED-36d; FJ-01 →
AN-31; FJ-03e → AN-30; FJ-06 → AN-15; FJ-07 → AN-24; FJ-11 → AN-11c; SG-06 →
AN-17; SG-09 → FJ-17; SG-13 → AN-34; SE-07 → SG-25; FI-14 → ED-41; FI-27 →
ED-33; FO-18 → ED-37; SD-02, SD-12 → FO-27; SU-12 → RC-18; SU-15 → SG-28;
CD-01 → AN-29; CD-04 → FT-34; CD-16 → FI-33. These are the long chains that
run forward across tiers: K-01 → RC-03, RC-08; RC-01, RC-02, RC-20 → ED-06b
→ SD-02, SD-12; FO-10 → FO-08b, SD-02 (part 1), SD-12; FT-12 → SD-02, SD-12;
FJ-01 → FJ-02 → FJ-03a…FJ-03f → AN-30, CD-12, SU-11; FJ-18 → FJ-10…FJ-13;
SG-04 → SG-02; SG-01, SG-02, SG-05 → SG-06 → SG-07, SG-08, SG-09; SG-06,
SG-08 → SG-27 → SG-28; SG-11 → SG-10 → SG-12; SG-13 → SG-14, SE-15, AN-34,
CD-13; FI-36 → FI-02d, FI-02f, FI-02g, FO-19; FT-30, FT-31 → SD-07, OC-06,
FO-14a, FT-25; ED-01 → ED-02, ED-03; ED-49 → ED-50 → ED-12, OC-15b, AN-24;
ED-26, ED-27 → OC-12; MS-01 lands with every P row; MS-13 (the corpus runs)
closes AN-01, SG-26, SD-01, SD-04, SD-11, FT-30, FT-31, CD-01 and CD-08.
