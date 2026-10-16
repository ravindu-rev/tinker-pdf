# Round 6 resume kit — drop this commit before integrating

This directory, and the `.claude/HANDOFF.md` note beside it, are one commit
on the branch `r6-wip`, made when the owner stopped round 6 in the cloud
session on 16 October 2026 (16:43 UTC) to continue it locally. The
container's scratch directory does not travel, so everything round 6 had only
there is packed here. **Drop this commit** (and fold the lane F checkpoints)
before anything reaches `develop`.

## What `r6-wip` holds

On `develop` = `d1c32cd` (origin), in order:

1. **Lane K, finished** (four signed commits, `35ab4ab`…`05cea0b`):
   - `35ab4ab`: the probe kit's known issues fixed;
   - `9dd8c73`: the kit's families as `#[ignore]`d ratchets against
     `935b077`, in `crates/tinker-pdf/tests/rtl_probes.rs` (families as
     modules under `rtl_probes/`) and `crates/tinker-pdf-shape/tests/rtl_lines.rs`;
   - `bec1ce1`: a committed case for every book the K-11 and CD-19 reviews
     named (fixed ones held, open ones pinned);
   - `05cea0b`: docs.

   Gates were green; **no independent review has run on it yet.**
2. **Lane F, stopped mid-lane:** sixteen signed `WIP:` checkpoints,
   `4af61f0`…`f74e164`. They are not proper commits; fold them, one commit
   per mechanism, before integrating.
3. **This kit.**

## The files here

- **`laneF-scratch.tar.xz`**: lane F's scratch directory (`r6/laneF`),
  without its build trees, snapshot copies and per-family dumps. It holds:
  - **`laneF/NOTES.md`**, lane F's own record. Read its end first.
  - `variants/`: every `paint.rs` variant tried, A … A9L, plus
    `bidi-H1.rs` and `bidi-H6.rs` for the owner options, and
    `laneF-injections.json`.
  - `tools/`: the run, compare, injection, timing and fold scripts
    (`both.sh`, `owner3.sh`, `inj*.sh`, `time*.py`, `mkfinal9l.py`,
    `stages.py`, `docs_edit.py`, …).
  - `out/*.txt` and `out/*.json`: the summary tables: `table-*`, `optsum-*`,
    `optkey-*`, `time8.txt`, `time10.txt`.
  - `msg1.txt` and `msg2.txt`: the drafted commit messages.

  The scripts hold absolute paths from the cloud container
  (`/tmp/claude-0/…/scratchpad/r6/laneF`, `/home/user/tinker-pdf`). Extract
  the archive to a scratch directory and adjust those paths.
- **`laneV-laneK-scratch.tar.xz`**: the source files (probe `.rs` files,
  scripts, notes, small JSON and TSV) from lane V's and lane K's scratch
  directories and from the verifier of lane V. Their outputs were too large
  to carry. Lane F imported lane V's families into the kit as `zz_v6_*`.
- **`journal-wf_db942488-334.json`**: the workflow's journal. Its three
  `result` entries are **lane K's report**, **lane V's findings** and
  **verify:laneV's verdicts**.
- **`rtl-r6.workflow.js`**: the workflow script that ran round 6. The review
  phase (hunt and code, then confirm, an adversarial verifier per finding)
  and the bounded fix loop (at most two rounds) can be reused as they are,
  given lane F's final result in the `LANE` schema.

## Where lane F stopped

- **At 16:37 it decided to commit A9L** (`variants/paint-A9L.rs`;
  `tools/mkfinal9l.py` applies it):
  - a drawing the search tries is taken at its letters, as at `935b077`;
  - the rules' letters reading is still searched in the runs' space under a
    transform.

  Measured on A9L, in a release build:
  - against `935b077`: 0 books worse in either build; 272 better (349 with
    `bundled-fonts`);
  - timing at n=256: right-to-left justified text 1.09× of `935b077`,
    letter-spaced and scaled 1.71×, the rest 1.04–1.20×.

  The A8 tip had cost 6.3×, and A9 2.57×.
- **Checkpoint `f74e164`** is A9L in the tree. It is the ninth checkpoint;
  `86fde67` was A9.
- **Gates** were all green on `899b5a8`: fmt; clippy in both builds and on
  1.99; tinker-pdf at 2 733 passed in the default and `bundled-fonts`
  builds and 2 727 with no default features; determinism in both builds and
  on wasm32-wasip1. Re-run them at the final tip.
- **Owner options measured:**
  - **H1** (the comma tie-break), against A8: `out/optsum-H1.txt`,
    `optkey-H1.txt`. Large gains, and costs in the H6, `.!` and `?!` opener
    class and the price books.
  - **H6**, against A8: `optkey-H6.txt`. 8 books worse and 12 better in the
    default build (6 and 10 with `bundled-fonts`); the H6 class goes from 8
    to 0.
  - **To re-measure against A9L:** H1 and H6 (as H1n and H6n), and D (each
    drawing read once; A9Ld).
  - **To measure:** H2-3 (Latin parity for an inline-block cutting a Greek,
    Cyrillic or Armenian word).
  - **Also owner options:** the A9, A8 and A8s searches, priced in time
    against A9L (W, W+J and W+J+C in NOTES.md).

## What is left, in order

1. **Finish lane F.**
   1. Finish A9L: injections for its rules, with counted fires; the cases
      that fail at `05cea0b` shown failing first.
   2. Measure the remaining owner options against A9L.
   3. Docs: ROADMAP CD-19 narrowed or closed, `epub.md`,
      `content-and-text.md`, and ruling 14 if a rule changed.
   4. Fold the WIPs into one signed commit per mechanism (`msg1.txt`,
      `msg2.txt`, `tools/stages.py`).
   5. Every ratchet suite in both builds, with its `RAN` banner, and the
      gates.
2. **Review:** hunt and code in parallel, then confirm; an adversarial
   verifier per finding; at most two fix rounds (the script above).
3. **Land it.** Put lane K and lane F's folded commits on `develop` without
   this kit commit, run the gates and push `develop`. Then:
   - put H1, H2-3 and H6 to the owner, with the measured prices;
   - run two workflows in parallel:
     - **`cd19-close`:** apply the owner's decisions and close or narrow
       CD-19;
     - **`ms11`:** `ci.yml` on push to `develop`; dispatch `fuzz.yml` and
       `corpus.yml` on `develop` and watch them to the end, each failure
       becoming a root-caused fix or a ROADMAP row;
   - rewrite `.claude/HANDOFF.md`.
4. **The owner deletes on GitHub:** `r6-wip` once it has landed, and the
   stale `rtl-probe-kit`, `wave8/8A-impl` and `wave8/8B-impl`.

## Owner decisions of 16 October 2026, to record in the repository

- **H-01, branch policy.** The owner's words: "that's fine you can do the
  changes directly to develop, i'll manually check and merge develop to
  master for releases".
  - Every change goes to `develop`, with no pull requests.
  - The owner alone merges `develop` → `main`.
  - Record it in ROADMAP's H-01 and MS-11 rows and in AGENTS.md "Branches
    and releases".
  - The schedules (nightly fuzz and corpus, weekly bench) run `main`'s
    code until the owner merges, so K-06's seven nights count only from
    then.
- **H-03, the corpus runs.** Authorised: dispatch `corpus.yml` (and
  `fuzz.yml`) on `develop` in GitHub Actions. The corpora are still never
  fetched outside CI. MS-13's censuses that have never run stay a separate
  item.

## Baselines

- `9297d2d` and `56dfd93` are ancestors of `develop`. Build each in its own
  worktree with its own target directory.
- The `935b077` baselines are committed in
  `tools/probes/rtl/baselines/935b077/`; `9297d2d` and `56dfd93` sit beside
  them.
