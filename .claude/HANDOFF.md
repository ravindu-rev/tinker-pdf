# Where the last session stopped

Written 23 October 2026, when round 6 of ROADMAP CD-19 landed on `develop`,
and brought up to date on 24 October 2026, when the roadmap was cut into
steps. The next session starts here: read this, then
[AGENTS.md](../AGENTS.md), then continue with "Next, in this order" below.
When this note is out of date, rewrite it or delete it in the same commit
that moves the work on.

## The roadmap, cut into steps (24 October 2026)

- **What changed.** [docs/ROADMAP.md](../docs/ROADMAP.md) is now the index:
  what a step is, how one runs, the order of work. Each tier's rows are in
  `docs/roadmap/<PREFIX>.md`, with each row's text kept verbatim and its
  steps under it. The people's acts are in `docs/roadmap/H.md` and the hard
  limits in `docs/roadmap/NG.md`. Every row ID is unchanged, so every
  reference to one still holds.
- **How far the cut got.** Every row is cut; the roadmap's "Where the rows
  are" counts each tier's rows and steps. An independent pass reviewed 16
  cuts against the code (CD-19, FJ-01, K-01…K-09, P-10…P-14); every other
  row says its steps are not yet reviewed.
- **New rows.** P-27, the shape crate's build script (below, "Small and
  measured"), and 28 new H rows, H-134 to H-161. CD-19's decisions are
  H-149 to H-158; H-160 buys ISO 15930-8 for PDF/X-5 (SD-11) and H-161
  obtains EN 16931 and the terms of its validation artefacts (SD-24b).
- **The owner's instructions** (AGENTS.md):
  - 23 October 2026: work runs as workflows, one roadmap step per run;
    commits are signed with the default ed25519 key in `~/.ssh` through
    the repository's git config; when a step is done, its changes are
    merged into `develop` fast-forward, `develop` is pushed, and every
    branch except `develop` and `main` is deleted.
  - 24 October 2026: the rows not yet cut are cut in the session, tier by
    tier, without workflows.
- **Signing on the Linux machine.** Its key is `SHA256:6fQ5We1x…`, not the
  one that signed the history before (`SHA256:TsJhk…`). GitHub marks a
  commit verified only when its key is registered on the owner's account as
  a signing key.
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
- **CI on GitHub has not run on `develop`:** `ci.yml` fires on `main` and on
  pull requests (MS-11).

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
- **Small and measured, not yet made** (now ROADMAP P-27.1 and CD-19.2):
  - **`perf(shape)`:** `crates/tinker-pdf-shape/build.rs`'s `bidi_class`
    calls `abbreviate` once per code point. Hoisting the call out of the
    loop took the build script from 35.0 s to 0.21 s, with `ucd.rs`
    byte-identical, as measured by a peer session.
  - **`fix(rtl)`:** `run.sh`, `diag.sh` and `inject.py` force
    `CARGO_BUILD_JOBS=3`. Cargo's default is faster on 8 CPUs.

## Known issues on Windows

- `cargo fmt --all` passes Windows's command-line limit (os error 206) in a
  deep worktree. Check each package with `cargo fmt -p`.
- A tree copied with `tar` keeps its files' times, so cargo can reuse a
  stale binary from a shared target. Never share a target between trees.

## Next, in this order

1. **ROADMAP's "Order of work"**, step by step:
   - CD-19.1 to CD-19.7: the design doc for what is left, the kit's fixes,
     the timing runner, the `ride=` ratchet, the review hunts' classes,
     `plan_key` by place, and Left (10)'s guard prototyped and priced;
   - the owner's word on Left (10) (H-149), then CD-19.8;
   - the options rebuilt on the landed code and re-timed (CD-19.9 to
     CD-19.13), the owner's choices (H-150 to H-154), CD-19.14 to CD-19.18,
     and the rest of CD-19 in its order;
   - then P-27.1, MS-11 and K-06, K-01 and K-02, MS-01, MS-04, MS-06 and
     MS-02, P-01 to P-05, and the tiers.
2. **Separately, as the owner asked on 24 October 2026**: the review of the
   cut rows whose steps are not yet reviewed. Until it happens, each is
   reviewed against the tree when it comes up ("How a step runs", 1).
