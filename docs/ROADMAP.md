# Roadmap

The goal, stated as a capability target: **every capability a mature,
widely deployed PDF engine offers, every capability the rest of the field
offers, and more — and every document a real producer emits opens, renders
provably the way the world renders it, round-trips, signs, conforms and is
searchable, on every target, deterministically, with no unsafe code, no
third-party logic and no silent failure, at a speed with a number behind
it.** The feature docs record what already holds; the roadmap lists what
does not yet, and nothing else. What has left it is in git history and in the
design docs' own "as built" sections, not here.

**What is in scope is settled; what comes first is measured.** On
9 October 2026 the owner decided that this engine has every capability the
field offers, and more: speed verified by tests and the engine optimised
against them; form JavaScript as a complete implementation; signing with a
private key in the engine; OCR in scope and last, as a later
implementation; and everything else such an engine offers. Every row of
the roadmap is in scope and none waits for a corpus to ask for it. Ruling 3
is amended, not retired ([rulings.md](rulings.md)): corpus hit-rate
evidence no longer decides *whether* a capability is built, it decides *the
order* inside a tier — the row a real document needs goes before the row
none has asked for yet. The tiers run in order: 0 (measure), 1
(correctness), P (performance), the capability tiers by area, container
depth, and OCR, which is last by the owner's instruction. Every manual
verification and every decision that needs a person is in
[roadmap/H.md](roadmap/H.md), "Human verification and owner
decisions", and nowhere else; a row
whose only remainder is a person's act moves there.

Nothing in the roadmap names another engine, viewer, validator or document
tool — the corpora included, each of which is named by its role. A
capability is described on its own terms; "the field" means what a reader
of PDFs is entitled to expect. Where a number was set by a planning-time
comparison, the comparison was run once, is a dated measurement kept
outside this repository, and is never re-run against a build (H-112); the
number stands on its own terms here and its exit criterion is first-party.

**Rows and steps.** The work is held at two sizes. A **row** is a
capability, gap or finding, as the replanning of 9 October 2026 wrote it
and as later work has amended it: its **Item**; its evidence, **Today** (a
measured number, a named refusal, or "absent — the owner's parity decision
of 9 October 2026" where nothing in the tree measures the gap, with the
search or the line that establishes it); its **exit criterion** (a
first-party test or CI job that runs — a row is not done when its code
lands, it is done when the check goes green); its design **Doc**, linked
where it exists and marked *owed* where it does not; and what it **Depends
on** — rows, and the person's acts in [roadmap/H.md](roadmap/H.md), the H
rows. Rows were sized in engine-months, S ≈ 0.5, M ≈ 1–2, L ≈ 2–4 and
XL ≈ 5–8, and that is too big for the way the work runs now: one workflow
run takes one piece of work and lands it (AGENTS.md, the owner on 23
October 2026). So each row is cut into **steps**, each one run's worth,
under the row's own text, which stays as it was written; "What a step is",
below, says what one holds. The rows at the front of the order of work
were cut first, on 23 and 24 October 2026, and an independent pass reviewed
sixteen of those cuts against the code (CD-19, FJ-01, K-01…K-09 and
P-10…P-14). The rest are cut in the session, tier by tier and without
workflows (the owner, 24 October 2026); "Where the rows are" counts each
tier's. A cut row no independent pass has reviewed says so, and so does a
row not yet cut. Every cut so far was made against the tree at `5297133`,
and where a row's text and that tree disagree, the row says so under its
steps. Every number and every `file:line` pointer in a
row was read from the tree on 9 October 2026, or carries its own date. A
number the roadmap carries and the tree contradicts is a defect in the
roadmap.

The measurements the tiers are ordered by, from `corpus/ratchet.json` and
its two font-bearing siblings:

| Measure | Value |
| --- | ---: |
| Corpus files (a viewer's test suite 974, an archival validator's corpus 2 907, a structural tool's suite 637, an association's examples 7, a web crawl 1 000) | 5 525 |
| Render every page | 5 516 |
| Do not (6 in the structural tool's suite, 3 in the web crawl; every one a file that would not open) | 9 |
| Rendered with something reported, no faces / synthetic face / bundled faces | 1 412 / 459 / 495 |
| `rotate` held of asked | 4 998 of 5 114 |
| `rotate-tight` held of asked | 4 583 of 5 114 |
| `crop` held of asked | 4 929 of 5 075 |
| `dpi` held of asked | 5 342 of 5 457 |

The suite stands at 6 819 passed, 0 failed, 63 ignored across 289 suites as
[verification.md](verification.md) records it, measured 9 October 2026 on
`x86_64-unknown-linux-gnu`.

## Where the rows are

One file per tier, under [roadmap/](roadmap/), named by the rows' prefix:
CD-19 is in [roadmap/CD.md](roadmap/CD.md), its third step is `CD-19.3`, and
`grep -n '| CD-19.3 |' docs/roadmap/CD.md` finds it. Each file opens with the
tier's own rules and history, then gives each row its text and its steps.

| Prefix | Tier | File | Rows | Cut into steps | Steps |
| --- | --- | --- | ---: | ---: | ---: |
| MS | Tier 0 — measure what is not measured | [roadmap/MS.md](roadmap/MS.md) | 13 | 13 | 66 |
| K | Tier 1 — prove correctness | [roadmap/K.md](roadmap/K.md) | 10 | 10 | 59 |
| P | Tier P — performance | [roadmap/P.md](roadmap/P.md) | 27 | 27 | 184 |
| RC | Capability tier — rendering and colour | [roadmap/RC.md](roadmap/RC.md) | 21 | 21 | 172 |
| FT | Capability tier — fonts and text | [roadmap/FT.md](roadmap/FT.md) | 43 | 43 | 380 |
| ED | Capability tier — editing, document operations and creation | [roadmap/ED.md](roadmap/ED.md) | 57 | 57 | 269 |
| AN | Capability tier — annotations, forms and XFA | [roadmap/AN.md](roadmap/AN.md) | 37 | 37 | 149 |
| FJ | Capability tier — form JavaScript (complete) | [roadmap/FJ.md](roadmap/FJ.md) | 23 | 23 | 213 |
| SG | Capability tier — signatures (private-key signing in the engine) | [roadmap/SG.md](roadmap/SG.md) | 28 | 28 | 129 |
| SE | Capability tier — security | [roadmap/SE.md](roadmap/SE.md) | 15 | 15 | 36 |
| FI | Capability tier — formats in | [roadmap/FI.md](roadmap/FI.md) | 50 | 0 | 0 |
| FO | Capability tier — formats out and print production | [roadmap/FO.md](roadmap/FO.md) | 42 | 0 | 0 |
| SD | Capability tier — standards | [roadmap/SD.md](roadmap/SD.md) | 26 | 0 | 0 |
| SU | Capability tier — surface: CLI, bindings, packaging, API stability, documentation | [roadmap/SU.md](roadmap/SU.md) | 24 | 0 | 0 |
| CD | Container depth | [roadmap/CD.md](roadmap/CD.md) | 19 | 1 | 37 |
| OC | OCR — the last implementation tier | [roadmap/OC.md](roadmap/OC.md) | 24 | 0 | 0 |
| H | Human verification and owner decisions | [roadmap/H.md](roadmap/H.md) | 133 | — | — |
| NG | Named non-goals | [roadmap/NG.md](roadmap/NG.md) | 14 | — | — |

## What a step is

A step is the unit of work: one workflow run takes it, lands it on
`develop` and finishes it.

- **One change, one mechanism**, merged into `develop` as one commit or a
  short series that only makes sense together. As a guide, it changes no
  more than about 400 lines of non-test code, in one crate or two adjacent
  ones, and a reviewer reads its diff in one sitting; tests, fixtures and
  transcribed standard data may add more.
- **Cut by mechanism, not by instance.** A step fixes or builds a whole
  class — every shape one mechanism produces, across scripts, directions,
  builds and document types — never only the shape someone reported.
- **Its own exit**: the first-party test or CI job that closes it, file and
  test name, and what that test asserts.
- **No open question inside.** A question that shapes the design is an
  earlier `design` step. A decision only a person can take is an H row the
  step depends on, and a price — anything a change costs that used to
  work — is never chosen inside a step.
- **Ordered and wired.** A row's steps run in number order. **Depends on**
  names earlier steps of the row; other rows, meaning the whole row; the
  step that creates a design doc several rows share; and H rows.
- **Together they close the row**: its last steps meet the row's exit
  criterion as the row states it.

| Kind | What the step lands | Its exit is |
| --- | --- | --- |
| `design` | a design doc in [design/](design/), or a section of one: scope, non-goals, design, milestones with concrete exit tests, risks, in that order | the doc section exists, names each later step's exit test, and is reviewed |
| `build` | new capability: code and its tests | named tests, failing first |
| `fix` | corrected behaviour | the failing test that shows the defect, then passing |
| `test` | conformance data, fixtures, a harness, a suite, transcribed annex vectors — no engine change | the suite runs and prints its `RAN` banner where it can skip |
| `measure` | a census, benchmark or probe whose committed output is a number a doc or the roadmap quotes | the committed output and the test or job that regenerates it |
| `ci` | a workflow file under `.github/workflows/` or an `xtask` job | the job exists and its run is green |
| `doc` | documentation only: a feature doc, an example, API docs | the doc, and any example compiles and runs |
| `watch` | an observation over time or a run read to completion — nights of CI, a corpus job on CI — and no code | the named run or runs green, read to the end |

## How a step runs

One run, one step, in this order. [AGENTS.md](../AGENTS.md) holds the
owner's standing instructions behind each of these.

1. **Pick** the first step in "Order of work" whose **Depends on** are all
   done: every step it names has landed, every row it names has left the
   roadmap, and every H row it names is decided or done. A row not yet cut
   is cut first, in the session, and its cut lands as its own commit to its
   tier's file, with the H rows it needs. A row whose steps are not yet
   reviewed has them reviewed against the tree first, and the corrections
   land before its first step.
2. **Concept first.** Name the mechanism and prototype the change, then run
   the cheap checks that decide it: does any book change, and what does it
   cost in time, in release and in debug?
3. **Build to the exit.** Tests fail first; each rule the step adds gets a
   counted injection ([CONTRIBUTING.md](../CONTRIBUTING.md), "Working an
   item"); both builds, default and `bundled-fonts`, wherever fonts or text
   are involved; the feature doc changes with the behaviour; a new
   capability brings its speed row (MS-04) and its determinism fingerprint
   (K-07).
4. **Review** by an agent with a fresh context that hunts new shapes, reads
   the code and re-measures every claim. Every finding is reproduced before
   it is fixed, and fixes run in a bounded loop.
5. **Stop for a price.** When the step would cost anything that used to
   work, the run records the figures and stops; the owner decides, through
   an H row, and their decision goes into [rulings.md](rulings.md).
6. **Land.** Commits are signed and carry no trailers (AGENTS.md).
   Everything the step changed is merged into `develop` fast-forward,
   `develop` is pushed, and every branch except `develop` and `main` is
   deleted — local branches, worktrees, and any side branch on `origin`.
7. **Record.** The commit that lands the step removes its line from its
   tier's file. When a row's last step has landed and its exit criterion
   is green, the row leaves in the same commit, with its feature doc's
   refusal row.

A step that turns out bigger than one run is never half-landed: the run
splits it in the roadmap first — its pieces take its place as `<step>a`,
`<step>b`, … — and takes the first piece.

## How the roadmap changes

A step leaves the roadmap in the commit that lands it, and its ID is never
reused. A row leaves when its last step has landed and its exit criterion is
green, and its feature doc's refusal table loses the matching row in the
same commit. There is one other way out and it is not a shortcut: a row
whose exit criterion can never go green, for a reason outside this
repository's power, is not work the roadmap is tracking, it is a **limit**,
and it moves to [roadmap/NG.md](roadmap/NG.md) with the reason it cannot
close. Design docs in [design/](design/) carry scope, non-goals, design,
milestones with concrete exit criteria, and risks, in that order; a row's
`design` step writes its part, and the row's later steps are that part's
milestones.

- **An exit criterion is a first-party test or CI job** (ruling 13). An
  outside program may host the code, execute it, fetch bytes for it or
  generate a fixture once; it never says whether an output is right. A
  planning-time comparison may set a target number and nothing else, and
  it is never named here.
- **Every new capability brings its speed row with it** into the benchmark
  set (MS-04) and its determinism fingerprint (K-07) in the step that
  closes it.
- **A row whose only remainder is a person's act moves to
  [roadmap/H.md](roadmap/H.md)**, leaving its machine half (if any) in its
  tier as steps.
- **Named non-goals hold hard limits only** — a reason outside this
  repository's power. A scope choice is a row, however late its tier.
- **Rows enter with evidence and with their steps**: a named refusal, a
  test, a corpus number, or "absent — the owner's parity decision of 9
  October 2026" where nothing in the tree measures the gap, followed by how
  the absence was established — the search that finds nothing, quoted so it
  can be re-run, or the line or refusal that says so — and the row is cut
  into steps in the commit that adds it.
- **A row is cut before work on it starts**, in the session, tier by tier
  and without workflows (the owner, 24 October 2026). The cut follows "What
  a step is" and lands as its own commit to its tier's file.
- **A step's plan follows the tree.** When the tree shows a step's plan is
  wrong, the step is rewritten or split in the commit that finds it; a
  row's exit criterion changes only by the owner's word.
- **Bands and floors move one way.** Lowering a ratchet floor or widening a
  benchmark band is a reviewed act (H-78).

## What "best" means here

The target is not a list of features but a set of axes, each with a number
that can only move one way. A capability the field offers and this engine
lacks is a row in a capability tier. An axis nobody measures is a row in
tier 0, and it comes first, because a claim with no ratchet behind it is
the kind this repository has already caught itself making.

| Axis | Measured today | What gives it a ratchet |
| --- | --- | --- |
| Correctness on documents nobody here wrote | 5 525 files in five pinned corpora — a viewer's test suite, an archival validator's conformance corpus, a structural tool's suite, an association's examples, and **1 000 documents a crawler found on the open web** spanning 462 distinct `/Producer` strings; 5 516 render every page | `corpus/ratchet.json`, nightly, every failure attributed by producer — **but the nightly fails on the stale default branch every night and has measured nothing of the October work** (MS-11, MS-13) |
| **Speed** | seven criterion operations on 3-page inputs, weekly, on the default branch only; `crates/tinker-pdf/benches/baseline.json` records each one's fastest figure on `ubuntu-latest` over fifteen dispatches of one revision, with bands of 35 % to 135 % over a two-cluster runner fleet; "render text at 150 dpi" draws no glyphs | **counted-work guards in `cargo test` on every pull request and target (MS-01); a paired wall-clock gate on every pull request against the merge base and against the planning baseline, which holds every tier-P target as a factor, a calibration or a relation with no person in the loop (MS-02); an absolute ratchet on a pinned reference runner (MS-03)** |
| Memory | 65 caps in `bounds_ledger.rs`, two of them the runtime bounds a *process* spends and one a relation between a symbol and its page rather than a quantity; every corpus child measures its own peak resident set and `ratchet.json` bands the per-corpus maximum within a **measured 2 %** tolerance | per-operation peak bands (MS-05) |
| Footprint | **6.76 MB of wasm, 3.28 MB gzipped** (9 October 2026), **over** `release.yml`'s 2.5 MiB gzipped gate, which only the release checks | **a CI ratchet on every pull request, per build profile (MS-06)**; the budget is the owner's (H-04) |
| Determinism | 15 render fingerprints and 3 document hashes on linux, windows, macos and `wasm32-wasip1` | extended to every subsystem the roadmap adds, parallel and cached paths included (K-07, MS-01's A9) |
| Fidelity | arithmetic fixtures, metamorphic relations, committed fingerprints; nine reviewed goldens never reviewed; ruling 13's amendment of 5 September 2026 on dated outside measurements | reviewed goldens (H-60) and the new families (K-08, H-73) |
| Conformance | PDF/A agreement 1 948 of 2 371 (measured before the October rules); PDF/UA 29 of 239 caught, 0 false alarms (before the facade); html5lib 1 779 of 1 784; CommonMark 651 of 652; UAX #14 19 338 of 19 338 | each suite's pass count ratcheted, and the ECMAScript conformance suite run to its end for the 2025 editions (FJ-01…FJ-03f); PKI path-validation data (SG-11), crypto vectors, the PDF object grammar (SD-20), PDF/A, UA and X (SD rows) |
| Robustness | 51 fuzz targets nightly; the nightly `jbig2` target fails on the default branch | fuzz on current code green (K-06), structure-aware and metamorphic targets (K-04), timeouts and memory blow-ups counted as failures (K-05) |
| Capability coverage | this file's rows | each row's exit criterion; open and closed counts per tier recorded at each wave's close |
| Surface | 221 C functions; six bindings — Python and JavaScript over the facade, and .NET, Go, Ruby and Java over the C ABI — with Swift as source nobody has compiled, and none projecting the whole facade; eighteen CLI subcommands | the SU rows; `cargo xtask bindings-coverage` (SU-12) |
| Maturity | version 0.0.1, nothing published, one release run watched | SU-17, SU-18 and the owner's release decisions (H-113, H-114) |

## Order of work

A step runs once everything its **Depends on** names is done. While the
next step waits on a person, the run takes the next one in this list that
does not, so the machine work never stalls on a decision; and a row not yet
cut is cut before its first step is taken ("How a step runs", 1).

**First, in this order**, each because it unblocks the most:

1. **CD-19**, first by the owner's word (10 and 23 October 2026; the row
   says what is left and why). Its groundwork first, CD-19.1 to CD-19.7:
   the design doc for what is left, the probe kit's own fixes (each tree
   its own cargo target, cargo's own job count), a timing runner, a ratchet
   for `ride=`, a class for the review hunts' books, the page's plans keyed
   by place rather than by address, and Left (10)'s guard prototyped and
   priced. Then the owner's word on Left (10) (H-149) and CD-19.8; the
   options rebuilt on the landed code and re-timed in release and debug,
   CD-19.9 to CD-19.13; the owner's choices (H-150 to H-154) and CD-19.14
   to CD-19.18; then the rest of the row in its order, to its close
   (H-158).
2. **P-27.1**, the shape crate's build script: measured and not yet made,
   and every build pays for it.
3. **MS-11**, CI on `develop` (MS-11.1 to MS-11.5) — whose `epub-corpus`
   job has shown that K-11's fix, closed 10 October 2026 on synthetic books
   alone since no corpus is fetched here, lets
   `sample-regime-anticancer-arabic.epub` conserve past character 20 963:
   it was green on 94e3bb5, run 38118684750, and on 56dfd93, run
   38138512898 — and **K-06**, the fuzz failure (K-06.1 to K-06.4; its
   seven nights count from the owner's next merge of `develop` into
   `main`), with H-02 and H-03.
4. **K-01** (the external graphics state) and **K-02** (default colour
   spaces; K-02.2 waits on H-135), because both change what every page
   draws.
5. **MS-01, MS-04, MS-06 and MS-02**, the speed and footprint instruments,
   in that order.
6. **P-01 to P-05**: algorithmic, small, a 3–30× win each on its operation.

Then the capability tiers in the order of "Where the rows are", each
ordered inside by corpus evidence (ruling 3 as amended), with signatures'
SG-01…SG-09 and form JavaScript's FJ-01…FJ-04 started as soon as their
owner decisions (H-32…H-36) are in; then container depth; OCR last.

**Dependencies that cross tiers.** *Depends on* overrides tier order: a
row waits for every row it names, wherever that row sits, and the named row
is pulled forward to meet it. These pull a later tier's row forward: SG-01
→ P-13; AN-06 → ED-36d; FJ-01 → AN-31; FJ-03e → AN-30; FJ-06 → AN-15; FJ-07
→ AN-24; FJ-11 → AN-11c; SG-06 → AN-17; SG-09 → FJ-17; SG-13 → AN-34;
SE-07 → SG-25; FI-14 → ED-41; FI-27 → ED-33; FO-18 → ED-37; SD-02, SD-12 →
FO-27; SU-12 → RC-18; SU-15 → SG-28; CD-01 → AN-29; CD-04 → FT-34; CD-16 →
FI-33. These are the long chains that run forward across tiers: K-01 →
RC-03, RC-08; RC-01, RC-02, RC-20 → ED-06b → SD-02, SD-12; FO-10 → FO-08b,
SD-02 (part 1), SD-12; FT-12 → SD-02, SD-12; FJ-01 → FJ-02 → FJ-03a…FJ-03f
→ AN-30, CD-12, SU-11; FJ-18 → FJ-10…FJ-13; SG-04 → SG-02; SG-01, SG-02,
SG-05 → SG-06 → SG-07, SG-08, SG-09; SG-06, SG-08 → SG-27 → SG-28; SG-11 →
SG-10 → SG-12; SG-13 →
SG-14, SE-15, AN-34, CD-13; FI-36 → FI-02d, FI-02f, FI-02g, FO-19; FT-30,
FT-31 → SD-07, OC-06, FO-14a, FT-25; ED-01 → ED-02, ED-03; ED-49 → ED-50 →
ED-12, OC-15b, AN-24; ED-26, ED-27 → OC-12; MS-01 lands with every P row;
MS-13 (the corpus runs) closes AN-01, SG-26, SD-01, SD-04, SD-11, FT-30,
FT-31, CD-01 and CD-08.

**The count**: 459 capability, gap and finding rows in the tiers, 275 of them cut into 1694 steps — MS 13 of 13 rows, 66 steps, K 10 of 10 rows, 59 steps, P 27 of 27 rows, 184 steps, RC 21 of 21 rows, 172 steps, FT 43 of 43 rows, 380 steps, ED 57 of 57 rows, 269 steps, AN 37 of 37 rows, 149 steps, FJ 23 of 23 rows, 213 steps, SG 28 of 28 rows, 129 steps, SE 15 of 15 rows, 36 steps, FI 0 of 50 rows, 0 steps, FO 0 of 42 rows, 0 steps, SD 0 of 26 rows, 0 steps, SU 0 of 24 rows, 0 steps, CD 1 of 19 rows, 37 steps, OC 0 of 24 rows, 0 steps; 14 hard limits (NG-01…NG-14); 133 rows for a person (H-01…H-159, H-134 onward added by the cut).
