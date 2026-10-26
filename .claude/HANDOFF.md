# Where the last session stopped

Written 23 October 2026, when round 6 of ROADMAP CD-19 landed on `develop`,
and brought up to date on 24 October 2026, when the roadmap was cut into
steps and the roadmap sweep landed. The next session starts here: read
this, then [AGENTS.md](../AGENTS.md), then continue with "Next, in this
order" below. When this note is out of date, rewrite it or delete it in the
same commit that moves the work on.

## The roadmap, cut into steps (24 October 2026)

- **What changed.** [docs/ROADMAP.md](../docs/ROADMAP.md) is now the index:
  what a step is, how one runs, the order of work. Each tier's rows are in
  `docs/roadmap/<PREFIX>.md`, with each row's text kept verbatim and its
  steps under it. The people's acts are in `docs/roadmap/H.md` and the hard
  limits in `docs/roadmap/NG.md`. Every row ID is unchanged, so every
  reference to one still holds.
- **How far the cut got.** Every row is cut; the roadmap's "Where the rows
  are" counts each tier's rows and steps.
- **Implementation first** (the owner, 25 October 2026: "each time a
  completion should only do the implementation and the test case writing,
  we can do a review and verififaction once all the things are done", and
  "no more workflow implementaions only multi agents"). A run reads
  `tools/roadmap/brief.py <STEP>`, never a whole tier file, builds the step
  and its tests, and runs the gates through `tools/gates.py`, a line per
  gate with each log kept. No review, no verifying agent and no counted
  injections inside a run, and no step's exit asks for one (26 October
  2026; AGENTS.md, "How work runs").
- **What the cut's review changed** beyond its rows: MS-01 gained a closing
  step, MS-01.9, that reads every guard green after the P rows that land
  them, so the 49 steps that needed MS-01's tally now wait on MS-01.8, not
  the whole row (as cut, MS-01 could never close); and an ordering was
  added wherever two steps edit the same code without one.
- **New rows.** P-27, the shape crate's build script (landed 25 October
  2026, P-27.1), and 30 new H rows, H-134 to H-163. CD-19's decisions are
  H-149 to H-158; H-160 buys ISO 15930-8 for PDF/X-5 (SD-11) and H-161
  obtains EN 16931 and the terms of its validation artefacts (SD-24b);
  H-162 decides what gives if MS-02's paired job cannot fit its 15 minutes,
  and H-163 how MS-04 times an operation whose stated input cannot fit the
  20–300 ms window.
- **The owner's instructions** (AGENTS.md):
  - 23 October 2026: one roadmap step per run; commits are signed with the
    default ed25519 key in `~/.ssh` through the repository's git config;
    when a step is done, its changes are merged into `develop`
    fast-forward, `develop` is pushed, and every branch except `develop`
    and `main` is deleted.
  - 24 October 2026: the rows not yet cut are cut in the session, tier by
    tier, without workflows; runs save tokens (below).
  - 25 October 2026: a run does its step's implementation and its tests
    and nothing more; review and verification run once, when all the work
    is done; never a workflow, several agents only where the work splits
    into independent parts (below).
  - 26 October 2026: the instructions carry nothing for reviewers or
    verifiers: AGENTS.md, CONTRIBUTING.md and ROADMAP.md lost theirs, and
    216 step exits lost "and is reviewed" or "reviewed by a single
    subagent".
- **Signing on the Linux machine.** Its key is `SHA256:6fQ5We1x…`, not the
  one that signed the history before (`SHA256:TsJhk…`). GitHub marks a
  commit verified only when its key is registered on the owner's account as
  a signing key.
- **The roadmap sweep** (24 October 2026, `e8a935f0`):
  `tools/roadmap/sweep.py` checks every step at once for the classes of
  defect the reviews found by hand, `selfcheck.py` holds each of its rules
  to a fixture, and `accepted.tsv` records each finding that is not a
  defect, with its reason; the README lists the checks and what they cannot
  see. What it found was fixed in that commit, in 230 rows, and its message
  gives the figures. It reports 68 findings, all accepted. Run it after
  every change to the roadmap ("How the roadmap changes").
  - **Four questions for the owner**, left as they are:
    - P-13.8 rewrites the private-key path's Montgomery core. It now waits
      on SG-01.6, not on all of SG-01, so it no longer waits on SG-01.8,
      which records H-67's side-channel review of that path. One option is
      that the review comes after P-13.8: SG-01.8 then waits on P-13.8, and
      through it on H-109 and H-112. The other is that a change to the core
      after the review needs the review again.
    - SU-12's closing line reads the .NET column from its CI leg, which
      H-104 names as its alternative. It puts H-104's check by hand, where
      an SDK exists, outside the exit. Does the CI leg alone close that
      column?
    - H-112's Blocks cell says "nothing; an input to H-77", yet MS-04.10,
      which measures the set on the named machine, waits on it; the cell
      now also names MS-04. Does MS-04.10 need H-112? It brings H-112 to
      182 steps.
    - The index says both "A row's steps run in number order" and "Pick
      the first step in Order of work whose Depends on are all done". The
      sweep reads the second: a step whose Depends on are done may run
      while an earlier step of its row waits. If number order is strict, a
      row's tail is only its trailing `watch` and `measure` steps, and a
      wait narrowed to a step after an earlier `measure` step (FJ-01.42
      after FJ-01.41) avoids less than `e8a935f0`'s message says.
- **The cut's working files** (the step JSON, its checker and renderer)
  stay in `.claude/worktrees/roadmap-cut` on that machine only, ignored by
  git. Nothing below needs them: every row is cut, and a row added later is
  cut directly in its tier's file in `docs/roadmap/`.

## Where the work continues

**On `develop`**, pushed. Every change goes to `develop` directly, and only
the owner merges `develop` into `main` (H-01). `main` is unchanged at
`1a9013c`.

- Round 6 ran on the owner's Windows machine from 16 to 23 October 2026,
  after a cloud session had stopped it.
- Its sub-branches are deleted: `r6-wip` and `rtl-probe-kit` on GitHub, and
  every local lane branch. The folded commits on `develop` are the record.
- The lanes' notes, measurements and scratch probes stay in `.scratch/` on
  that machine only (gitignored). Nothing below depends on them.

## State

- **Round 6 on `develop`**: 20 signed commits after `d1c32cd`, up to
  `dedd99a`, then this note.
  - **Lane K** (`35ab4ab`…`05cea0b`): the probe kit's families are
    `#[ignore]`d ratchet suites against 935b077.
  - **`30d92c5`:** the kit names its fixtures with `/` on every platform,
    so the suites hold on Windows.
  - **Lane F, CD-19's fix** (`fcdba59`…`ade0b2e`): each right-to-left line
    is read back on the page it is drawn on. It costs no time: release
    `zz_v6_time` runs at 0.24–0.91× of 935b077 on every shape (`ls-scaled`
    0.90–0.95×), and debug at 0.20–0.80×, against a noise band of about
    ±7%. `read_as` is narrowed, so the NO-BREAK SPACE grid reads `?!` as
    typed. The owner declined three prices, and ruling 14 records each with
    its figures: A9L's time and `read_as`'s text on 17 October; the release
    `ls-scaled` time and NOOWN's books on 23 October.
  - **The review's fixes:**
    - `d134267`: a drawing found in the runs' own space is taken where it
      keeps more of the line's runs in their order. 19 paragraphs (8 with
      `bundled-fonts`) are back at 935b077's order, and 9 more read in
      order.
    - The kit's harness: `494f38e` (an injection's count comes from the
      bytes on disk and every binary started), `eba8649` (a cut run fails
      its comparison), `40862d7` (a TSV row needs a verdict) and `f280daf`
      (Left (9) has a class).
    - `a26d92d`: the review's hunts are kit families, `zz_rv6_hunt` and
      `zz_rv6_ib`.
    - `60d6426`: F2's books are pinned.
    - `dedd99a`: the owner's decisions (AGENTS.md, `.claude/settings.json`,
      and ROADMAP's H-01, H-03, MS-11 and K-06).
- **Gates at the landed tip (`dedd99a`), on Windows.** Every step exits 0:
  - fmt, checked package by package, all 22;
  - clippy, in the default and `bundled-fonts` builds;
  - `cargo test --workspace`: 6 994 passed;
  - `tinker-pdf` with `bundled-fonts`: 2 748 passed; with no default
    features: 2 742;
  - determinism in both builds, and the shaping fingerprints;
  - the seven examples;
  - wasm32-unknown-unknown and the four wasm32-wasip1 legs;
  - the fuzz check and `xtask check`.

  The only exception is the suites, below.
- **The suites** print 80, 80 and 4 `RAN` lines. The 78 older families and
  `rtl_lines` pass. `zz_rv6_hunt` (8 books) and `zz_rv6_ib` (15) fail in
  both builds, on CD-19 Left (10) alone: the owner landed them red on 23
  October as its tripwire.
- **CI on `develop`:** `ci.yml` fires on every push to `develop` since
  MS-11.1 (25 October 2026); MS-11.2 reads its first run to the end (H-02).

## Still open

Every CD-19 item below is now a step or an H row of ROADMAP CD-19 (CD-19.1
to CD-19.37, H-149 to H-158), in the order "Next" gives.

- **CD-19 Left (10), first in the order of work.** A dir-less Arabic
  paragraph with a styled `?!` in an inline block widened by `scaleX(2)`
  reads in logical order since round 6, where 935b077 did not. It draws two
  more glyphs over others (11→13, 6→8, 2→4), and one where none were drawn
  turned a quarter: 23 rows a build. The owner has to say whether that is a
  price for the order or wants a guard on the page. Held by the two red
  families above, and pinned by `epub_fallback.rs`'s
  `a_styled_mark_in_a_widened_block_reads_in_order_and_is_pinned_at_what_it_draws_over`.
- **CD-19's owner options**, none taken. Books worse/better against the
  tip, default build (`bundled-fonts` in brackets). Each was built on
  `6f4a8e12`'s `paint.rs`, whose kit outputs equal the landed tip's: rebuild
  each on the landed code before deciding (`d134267` changed
  `reads_closer`).

  | Option | What it is | Worse / better | Cost |
  | --- | --- | --- | --- |
  | H1 | the comma tie-break | 1 075 / 871 (1 775 / 916) | 6 case tests flip |
  | H6 | mixed stop runs | 8 / 12 (6 / 10) | — |
  | D | each drawing read once | 6 / 341 (7 / 257) | 4 more glyphs drawn over others |
  | W | hold each drawing tried to the line's words | 0 / 360 (0 / 333) | time only |
  | WJ | W, and search on where the rules' drawing runs two words together | 0 / 2 166 (0 / 1 784) | time only |
  | WJC | WJ, and search on where it cuts a word at the spacing | 0 / 3 903 (0 / 3 710) | time only |
  | H2-3 (i) | give the moved glyph back | 279 / 75 (189 / 24) | 174 (119) more books overprint |
  | H2-3 (i2) | its narrower form | 24 / 75 (8 / 24) | 24 (8) more books overprint |
  | H2-3 (ii) | a reader-side join | 24 / 189 (24 / 202) | glues Greek and Cyrillic words in plain PDFs |
  | H2-3 (iii) | one text object across the box | 0 / 370 (0 / 370) | class 93 → 18 (30 → 6); no price measured |

  W, WJ and WJC cost time. It was measured only in debug, against the
  fix's first form (A9L), which itself took 1.06–1.13× of 935b077's time:
  - W: `rtl-justified` 2.3–2.7×;
  - WJ: `rtl-justified` 5.8–6.6× and `rtl-justified-scaled` 2.1–2.2×;
  - WJC: a letter-spaced line 48–51×, and 69–72× under `scaleX(2)`.

  Re-measure each on the landed code, in release and debug.
- **CD-19 Left (1)–(9):** see ROADMAP CD-19. Each names its test and what
  it waits on.
- **The review's leftovers:**
  - `paint::plan_key` keys the page's plans by memory address. It is safe
    as called today, but fragile; keying by the run's place in the page
    would end that.
  - No ratchet holds `ride=`, the marks drawn outside their letter.
  - In the new families, 33 books (19 with `bundled-fonts`) read worse
    than at 9297d2d, though none worse than at 935b077, and `classes.py`
    names none of them. 23 (12) are `narrowmark` books like Left (7), 8 (4)
    are `both0.8`, and 2 (0) are `ar/fmt/rtl` turned a quarter.
- **The kit:**
  - `inject.py` touches only the crates its injections name, so give each
    tree its own cargo target.
  - The suites' times swing with load; the docs quote a quiet machine.
- **Small and measured, not yet made** (now ROADMAP CD-19.2):
  - **`fix(rtl)`:** `run.sh`, `diag.sh` and `inject.py` force
    `CARGO_BUILD_JOBS=3`. Cargo's default is faster on 8 CPUs.

## Known issues on Windows

- `cargo fmt --all` passes Windows's command-line limit (os error 206) in a
  deep worktree. Check each package with `cargo fmt -p`.
- A tree copied with `tar` keeps its files' times, so cargo can reuse a
  stale binary from a shared target. Never share a target between trees.

## Next, in this order

**ROADMAP's "Order of work"**, step by step:

- CD-19.1 to CD-19.7: the design doc for what is left, the kit's fixes,
  the timing runner, the `ride=` ratchet, the review hunts' classes,
  `plan_key` by place, and Left (10)'s guard prototyped and priced;
- the owner's word on Left (10) (H-149), then CD-19.8;
- the options rebuilt on the landed code and re-timed (CD-19.9 to
  CD-19.13), the owner's choices (H-150 to H-154), CD-19.14 to CD-19.18,
  and the rest of CD-19 in its order;
- then MS-11 and K-06, K-01 and K-02, MS-01 (to MS-01.8),
  MS-04, MS-05, MS-06 and MS-02, P-01 to P-05, and the tiers.

## The MS push, stopped on 26 October 2026

The owner asked for tier MS's implementation, then on 26 October to stop
once the steps under way had landed ("lets stop and complete the onlgoing
steps only we can continue later"), with one gate run over them all
rather than one a step ("after implementation run 1 gate not gate per
step"). Landed on `develop`, last at `97a016c` (11 gates ok over the last
four together): MS-11.1, MS-11.4, MS-11.5, MS-09.1, MS-08.1, MS-06.1,
MS-01.1, MS-04.1, MS-07.1, MS-02.1, MS-04.2, MS-07.2, MS-01.2, and K-02.1,
K-02.2, K-02.4, P-05.1, P-05.2, which MS steps wait on.

- **Next, needing no person** (each waits on the one before it):
  MS-04.3 to MS-04.8, then MS-05.1 (after MS-04.6) and MS-12.1, MS-12.2;
  MS-03.1 and MS-02.2;
  K-02.3, then MS-01.3 to MS-01.7; P-05.3, then MS-08.2; MS-07.3.
- **Waiting on the owner:** H-02 holds 31 MS steps, H-109 18, H-112 8 and
  H-110 4. Open choices: H-162 (MS-02's 15 minutes cannot hold), H-78 (the
  floor check MS-02.1 proposes), H-135 (inside a `/DeviceCMYK` group a light
  render keeps `k`'s own ink under `/DefaultCMYK`, as before; a guard row
  in `default_spaces.rs` pins it), and MS-04.7's container fixtures timed
  per format (6 operations) or per file (90).
- **Found, with no row yet:** JPEG and JPEG 2000 images never read
  `/ColorSpace`; Annex F's head path builds a page without its ancestors'
  `/Rotate` and `/CropBox`; `CosDocument::bytes()` fetches a streamed file
  whole without warning `WholeFileFetched`; `ScanIndex::build`
  (`crates/tinker-pdf-cos/src/repair.rs`) is quadratic in a file's
  objects, 13 s for 20 000 in release, a slow-input risk.
- **On the Linux machine**, builds run with `CARGO_PROFILE_DEV_DEBUG=0`:
  with debug info one full gate run built about 110 GB of test binaries a
  worktree, and parallel worktrees filled the disk.
- `cd-19.1` is a local branch only: an ended session's CD-19.1 draft
  (`0215252`), not merged.
