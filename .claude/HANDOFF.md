# Where the last session stopped

Written 12 October 2026, when the owner paused the work. The next session
starts here: read this, then [AGENTS.md](../AGENTS.md), then continue with
"Next, in this order" below. When this note is out of date, rewrite it or
delete it in the same commit that moves the work on.

## Where the work continues

**On the branch `rtl-probe-kit`, not on `develop`** (the owner, 12 October
2026). It starts from `develop` at `935b077` and adds:
- the probe kit;
- this note;
- a line in AGENTS.md pointing here.

`develop` was left as it is. Push the next session's work to
`rtl-probe-kit` until the owner says otherwise.

## State

- **`develop`** (`935b077`) carries round 5 of ROADMAP CD-19 and the K-11
  follow-ups: eight signed commits, from `c7e9b89` to `935b077`. The probe
  kit, [tools/probes/rtl](../tools/probes/rtl/README.md), is on
  `rtl-probe-kit` only. Every gate was green on
  that code, in the default, `bundled-fonts` and no-default-features builds,
  and no determinism hash moved.
- **`main`** is still the merge of `56dfd93`. By the owner's standing
  instruction, nothing goes to `main` and no pull request is opened until
  the owner says so.
- **Merging back:** whether and when `rtl-probe-kit` merges into `develop`
  is the owner's call.
- **What closed in round 5:**
  - CD-19's root: the standard-14 overflow font is written at `0 Tw`, so
    nothing after its code-32 letter moves.
  - H4, where search lost a word to CD-19's overprint, is gone.
  - V1: closing quotes and runs of stops end a line. The owner accepted the
    full price on 12 October 2026, and ruling 14 records it with its figures.
  - V3: a sign past WinAnsi reads with its number.
  - The reader's word cut: a word continues across a cut only where spacing
    put the next glyph.
- **Not run:** the workflow was stopped right after its second fix round, as
  the owner asked. That fix (`935b077`, "read each right-to-left line back as
  a reader does") has had no independent verification and no fresh hunt.

## Still open

The ROADMAP CD-19 row stays, narrowed. Its "Left" list (1)–(5) holds every
item below, with the counts measured on `935b077`.

- **H1, needs the owner's word on the comma tie-break.**
  - Shape: a Hebrew paragraph with no `dir` (or `dir=ltr`) ending in a
    number and a stop, under `word-spacing`, such as
    `<p>בראשית. אור ٦٠.</p>` at 0.5em.
  - Cost: 56 books in a default build, 53 of which conserved at `9297d2d`.
  - No drawing reads it right; the tie-break decides it.
- **H2-3, needs the owner's call on Latin parity.**
  - Shape: an inline-block cutting a Greek, Cyrillic or Armenian word.
  - Cost: 93 (30 with bundled fonts) books that read whole only because the
    overprint closed the block's gap.
  - A Latin word cut the same way reads as two words at every tree.
- **H6 with no spacing, needs the owner.** A right-to-left line opening with
  `.!` or `!…` after a `<br/>`: 4 (4) books plus 4 (4) minimised ones. The
  owner's 12 October decision does not name these.
- **H2-2 remainder and H2-7a–d, need reader changes.** About 50 books where
  no drawing reads the line right under K-11's reader:
  - a styled Greek word, then a bare right-to-left gloss and a Latin tail at
    2em;
  - a Latin gloss;
  - table cells;
  - `25، <b>W</b>`;
  - a dir-less Hebrew line ending in `؟`.
- **Smaller items:**
  - two hunt books per build;
  - 26 (25) dir-less table books that lose structure-tree order only (none
    ever conserved);
  - `5°<i>W</i>.`: 2 (2) books.

## Known issues in the probe kit

The kit's own verification, run from its README on a fresh checkout,
reproduced the `935b077` baselines byte for byte. It also found these, left
for the next session:

1. **Ctrl-C does not stop a run.** `run.sh`'s INT trap never fires while
   cargo runs, so Ctrl-C skips one family and still writes the done markers.
2. **`RV4C_FILTER` is not unset.** `run.sh` leaves it set, and
   `zz_vx_rv4code` reads it. The README's list of filters omits it.
3. **A false provenance line.** The README, the baseline note lines and
   `c4c456e`'s message say the `935b077` baseline ran on `7f1b3f8`, whose
   "paint.rs differs in doc comments only". In fact the two commits' `crates/`
   trees are identical; they differ only in `.claude/`, `.gitignore`,
   `AGENTS.md` and `CLAUDE.md`.
4. **Partial runs are misreported.** `table.py --by group` on a partial run
   reports the families it did not run as changed (fixtures, `LINE`). Use
   `--families` until this is fixed.
5. **The README's data-line table is incomplete.** It omits `VOV`, `TALLY`
   and the td probe's `ZZ` lines, and `rtlkit.Output` does not parse them,
   so the comparators would miss a change in them.
6. **Nits:**
   - `--help` is broken in `price.py` and `classes.py`;
   - the README leaves the `progress` file out of its output list;
   - `run.sh` creates an empty `lines/` directory;
   - nothing outside the kit's README mentions the kit.

The 77 tests in `probes/regressions/` are the verifiers' repro tests. Each
asserts what its finding said a book did at the tree its verifier used, so
29 fail at `935b077` in the default build and 26 with `bundled-fonts`. That
is expected. Read them finding by finding before turning any into a guard.

## Next, in this order

1. **Fix the kit's known issues above.** Then turn the probe kit into
   `#[ignore]`d suites, per AGENTS.md, so every family runs with
   `cargo test -- --ignored` and prints a `RAN` banner. Commit a test for
   every case the families found, even the small ones.
2. **Run the round-2 verification that was skipped,** and a fresh hunt, on
   this branch. Both baselines apply: `9297d2d` and `56dfd93`.
3. **Put the three owner decisions above to the owner:** H1, H2-3 and H6.
4. **Fix what remains by class,** not by instance (AGENTS.md): one lane for
   the reader-side mechanism behind H2-2 and H2-7.
5. **Then the roadmap's order of work:**
   - MS-11: `ci.yml` on push to `develop`, and the nightly fuzz and corpus
     runs dispatched on `develop`. Its epub-corpus clause is already met.
   - K-06: seven green nightly `jbig2` fuzz runs.
   - K-01, K-02.
   - The rest of Tier 0.
