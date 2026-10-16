# Where the last session stopped

> **16 October 2026, 16:43 UTC: round 6 was stopped in the cloud session**
> so the owner could continue it locally. It sits on the branch `r6-wip`.
> **Read [.r6-resume/README.md](../.r6-resume/README.md) first.** It says
> what that branch holds, where lane F stopped, what is left, and the owner's
> decisions of 16 October (H-01, H-03). The note below is older: its kit
> issues and its first two "Next" items are done on `r6-wip`. Rewrite this
> file when round 6 lands.

Written 12 October 2026, when the owner paused the work. The next session
starts here: read this, then [AGENTS.md](../AGENTS.md), then continue with
"Next, in this order" below. When this note is out of date, rewrite it or
delete it in the same commit that moves the work on.

## Where the work continues

**On `develop`.**
- On 12 October 2026 the owner paused the work on a side branch,
  `rtl-probe-kit`.
- On 16 October the owner had it brought back. It was fast-forwarded into
  `develop` (`b9b6970`, signed hashes kept), and its local copy was deleted.
- The remote `rtl-probe-kit` still has to be deleted by the owner on GitHub,
  with the stale `wave8/8A-impl` and `wave8/8B-impl`: this session's proxy
  refuses branch deletion. Nothing is lost by deleting them: `rtl-probe-kit`
  is an ancestor of `develop`, and the two `wave8/*` branches' work reached
  `develop` by cherry-pick in October.
- Round 6 picks up from "Next, in this order" below.

## State

- **`develop`** carries round 5 of ROADMAP CD-19 and the K-11 follow-ups:
  eight signed commits, from `c7e9b89` to `935b077`. It also carries the
  probe kit, [tools/probes/rtl](../tools/probes/rtl/README.md). Every gate
  was green on that code, in the default, `bundled-fonts` and no-default-features builds,
  and no determinism hash moved.
- **`main`** is still the merge of `56dfd93`. By the owner's standing
  instruction, nothing goes to `main` and no pull request is opened until
  the owner says so.
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

## Known issues in the probe kit: fixed

The kit's own verification, run from its README on a fresh checkout,
reproduced the `935b077` baselines byte for byte, and found six issues in the
kit. Round 6's lane K fixed all six on 16 October 2026, in the commit that
changed this paragraph; its message has the measurements:

1. an interrupt now stops the whole run and writes no done marker, however
   it arrives (Ctrl-C, a signal to the runner, or one to cargo or the test
   binary alone, which used to cost one family and still write the markers);
2. the runner unsets every variable a probe reads, `RV4C_FILTER` included,
   read off the probes;
3. the `935b077` baseline's provenance says what is true: `7f1b3f8`'s
   `crates/` tree is `935b077`'s;
4. `table.py` and `cmp.py` leave out a family a partial run did not run;
5. `VOV`, `TALLY`, `ZZ` and `PANIC` lines are parsed and compared;
6. every script answers `--help`, the README names `progress`, `lines/` is
   made only for a line probe, and `docs/verification.md` points at the kit.

Every book a finding of the K-11 round-4 review and of round 5 named, the
77 regression tests' among them, is a committed case in the test of its
mechanism, read as the kit reads a book (`epub_conservation.rs` and
`epub_fallback.rs`, 245 cases in each build; the kit's README, "The
regressions", says which test carries which): a case fixed is held to its
right reading, a case still open is pinned at today's and names the ROADMAP
CD-19 item that leaves it, so that it fails when that item closes. The
families are `#[ignore]`d ratchets against `935b077`, each printing a `RAN`
banner: `crates/tinker-pdf/tests/rtl_probes.rs` (63 families) and
`crates/tinker-pdf-shape/tests/rtl_lines.rs` (4); see the kit's README, "The
suites". `probes/regressions/` is gone.

## Next, in this order

1. **Run the round-2 verification that was skipped,** and a fresh hunt, on
   `develop`. Both baselines apply: `9297d2d` and `56dfd93`.
2. **Put the three owner decisions above to the owner:** H1, H2-3 and H6.
3. **Fix what remains by class,** not by instance (AGENTS.md): one lane for
   the reader-side mechanism behind H2-2 and H2-7.
4. **Then the roadmap's order of work:**
   - MS-11: `ci.yml` on push to `develop`, and the nightly fuzz and corpus
     runs dispatched on `develop`. Its epub-corpus clause is already met.
   - K-06: seven green nightly `jbig2` fuzz runs.
   - K-01, K-02.
   - The rest of Tier 0.
