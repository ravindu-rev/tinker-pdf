# Right-to-left probe kit

The probes the K-11 and CD-19 rounds wrote to measure right-to-left text in
EPUB books, the scripts that run and compare them, and their outputs at three
trees. They lived in one machine's scratch directory until now; they are
committed so that the next round starts from the repository and not from a
directory a container restart can lose ([AGENTS.md](../../../AGENTS.md), "A
test for every case").

Nothing here is compiled by cargo. The `.rs` files sit outside every crate's
`tests/` directory and `tools/probes` is not a workspace member, so no build,
test, clippy or fmt run sees them. The runner copies a probe into a checkout's
`crates/<crate>/tests/`, runs it there, and removes exactly what it copied.
Turning the families into `#[ignore]`d suites that print a `RAN` banner, as
AGENTS.md asks of every probe family that found something, is the next step;
see [What comes next](#what-comes-next).

## What a probe measures

A `tinker-pdf` probe builds synthetic books with the integration tests' own
helpers (`epub_support::book::styled_book`: a language, a stylesheet, a body),
opens each as a `Document`, and prints one line per book. It never fetches
anything: every book is built in the probe, from word lists in its source.

Most families print the round-3 review's line, which the kit calls an rr3m
line:

```text
ROW  <style>  <name>  m=  text=  search=a/b  ltr=  cons=  logical=  ride=  overlap=a/b  lines=  maxx=  thash=  chash=  <text read>  <divergences>  nf=
```

- `cons`: content conservation, the conservation suite's verdict
  (`epub_support::conservation::conservation`): the book's text comes back
  from the PDF.
- `logical`: the same read in logical order (`conservation_in_logical_order`,
  ruling 14).
- `text`: read whole, every block's text found whole in the page text.
- `search=a/b`: of the book's `b` words of two letters or more, `a` are found
  by `Page::search`.
- `overlap=a/b`: `a` glyphs that start before the one before them ends, in
  content order; `b` visible glyphs drawn over another on one baseline, the
  overprint CD-19 is about.
- `m` says the book holds combining marks and `ride` counts marks drawn
  outside their base; `thash` and `chash` hash the text read and the content
  streams, so two trees that write a book byte for byte alike say so.

A book **conserves** when every one of `content`, `logical` and `cons` its line
carries is true.

The older K-11 probes print `ROW <key fields> content=true|false
logical=true|false [search=...]`. Beside `ROW` the families print:

| Line | Family | Says |
| --- | --- | --- |
| `VOVX` | `zz_vx_*` | per book (style, language, hash of the body): visible glyphs drawn over another |
| `BOOK` | `zz_vx_k11rv_dirless` | whether each dir-less book's lines hold (`holds=`), with a `LINE` per misread line |
| `FAIL`, `rows=` | `zz_vx_probe_paren` | the books whose bracketed run and full stop do not conserve, and the total |
| `FIX` | `zz_k11rv_fixtures` | every local fixture's text in both orders and its content streams, hashed |
| `TWROW` | `zz_c1_tw` | strings the overflow font shows, those holding code 32 (`bx32`), those shown at a word spacing other than zero (`twbad`), overprint (`vov`), `cons`, `logical` |
| `ZZN` | `zz_code1_neg` | glyphs drawn over another under a negative word spacing |

The `tinker-pdf-shape` line probes read lines rather than books, through the
crate's `bidi` module: ruling 14's reading of the whole of
`BidiCharacterTest.txt` with the paragraph direction read off the line
(`bct-auto.tsv`), wrapped sentences on typed-name and mirrored-name pages
(`wrapped.tsv`, `sent.tsv`), and lane R's lines (`laneR.tsv`); one row a line,
`OK` or `NO`, with the text and its reading. `zz_fv_shape` prints `LINE` for a
handful of lines. `zz_code1_tdprobe` builds content streams by hand, reads
them through `TextDevice` and ruling 14, and writes one TSV row a case.

The timing probes (`zz_code1_scale`, `zz_h2scale`, `zz_code1_time`) print
wall-clock times while they double a line or a book. A time is not a
measurement another machine can repeat, so they have no baseline; read the
ratio between one size and the next.

## The families

[families.tsv](families.tsv) lists every probe the runner knows, its crate,
group, test threads and kind. Books a build at 935b077:

| Group | Families | Books | What they sweep | Written by |
| --- | --- | ---: | --- | --- |
| `kit` | `zz_rr3m_probe`, `zz_rr3m_extra`, `zz_rr3m_tie`, `zz_k11rv_fixtures` | 11 646, and 107 fixtures | marks under spacing in both orders, the bracket tie-break, and every local fixture | K-11 round 4 |
| `vx` | nine `zz_vx_*` | 21 752 | K-11's shapes from rounds 1 to 4 and the code review's table characters, each with the overprint shim | the round-5 review |
| `tw` | `zz_c1_tw` | 2 700 | the overflow font's code 32 and word spacing in its content streams | the round-5 review |
| `hunt` | seventeen `zz_rr3m_hunt_*` | 7 264 | brackets, numbers, URLs, glosses, isolates, pointed Hebrew, scripts, spacing, punctuation, code 32, gaps, cells, sweeps of `ET` and of dir-less books, minimised books | the K-11 round-4 review |
| `fv` | `zz_rr3m_fv_extra`, `zz_rr3m_fv_marks` | 7 056 | the shapes the 56dfd93 fix changed, and marks | the 56dfd93 verification |
| `laneR` | `zz_rr3m_laneR` | 15 523 | closing quotes and stops opening a line (K-11 V1), with `price/` and `soft/`, ruling 14's price | lane R |
| `h1` | eleven `zz_rr3m_h1*` | 8 539 | every overflow script, tables, glosses, marks, inline spacing, a breadth sweep, minimised books | the round-5 review |
| `h2` | sixteen `zz_rr3m_h2_*` | 15 176 | faces, gaps, nested levels, pages, tables, glosses, inline blocks in words, ornate brackets, a styled `?!` glued to a word | the review of the first CD-19 fix |
| `neg` | `zz_code1_neg` | 8 | negative word spacing | the round-5 review |
| `td` | `zz_code1_tdprobe` | 12 084 cases | content streams built by hand | the round-5 review |
| `lines` | `zz_k11rv_bidi`, `zz_k11rv2_sent`, `zz_laneR_lines`, `zz_fv_shape` | lines: 91 616 of the conformance file, 2 235 + 13 300 of sentences, 36 724 of lane R's, and 8 | ruling 14 over the conformance file and over wrapped sentences | the K-11 reviews, lane R |
| `timing` | `zz_code1_scale`, `zz_h2scale`, `zz_code1_time` | | linear time | the round-5 review and the CD-19 fix review |

Each `zz_vx_*` family is an earlier family with a shim around
`styled_book` that prints a `VOVX` line a book: `zz_vx_k11rv_ext` is
`zz_k11rv_ext` and so on, and `zz_vx_rv4code` is the round-4 code review's
`zz_rv4code`. At 935b077 their `ROW`, `BOOK` and `FAIL` lines are byte for
byte those of the families they wrap, in both builds, so the originals are not
kept.

[probes/regressions/](probes/regressions/) holds the minimised tests the
verifiers wrote to confirm or refute each finding, eight files and 77 tests
([The regressions](#the-regressions));
[probes/diag/](probes/diag/) holds one diagnostic probe. The probes are
committed as they ran: their opening comments still call them scratch, which
is what they were when they produced the baselines.

## Two baselines and the tip

Every change to right-to-left text is measured against two trees and today's:

- **9297d2d**, the tree before K-11's round-4 work. What conserved, read whole
  or was found by search there is what a change may not lose.
- **56dfd93**, K-11's fix after the review of its third review: the reader's
  rules everything since was built against. What conserved there must still
  conserve.
- **935b077**, today's `develop`, the second CD-19 fix: what this change
  moved.

A book worse now than at either baseline is a regression, unless it is in
ruling 14's price, which the owner accepted on 12 October 2026, or the owner
accepts it ([AGENTS.md](../../../AGENTS.md), "The owner decides prices").
ROADMAP CD-19's Left list names, by class and count, every book 935b077 leaves
worse; `classes.py` sorts a run's books into those classes and lists the ones
no class claims. Both builds, always: the default and `--features
bundled-fonts`.

## Running

The runner needs `bash`, `python3` and the checkout's own toolchain. From the
repository root:

```bash
# One family, default build:
tools/probes/rtl/scripts/run.sh . default /tmp/rtl/now 'zz_rr3m_hunt_min$'
# One group, both builds:
tools/probes/rtl/scripts/run.sh . both /tmp/rtl/now '^hunt/'
# The whole kit, both builds:
tools/probes/rtl/scripts/run.sh . both /tmp/rtl/now
```

The filter is an extended regular expression matched against `group/family`.
The first argument is any tinker-pdf checkout, so the same command probes
another worktree. The runner exports `CARGO_INCREMENTAL=0` and turns debug
information off, unsets the probes' own filters (`RV_FILTER` and the rest),
and deletes each probe's test binary after its run (`KEEP_BINARIES=1` keeps
them). The run that produced the 935b077 baseline took about an hour a
build.

Each family runs on its own: one `cargo test -p <crate> --test <family> --
--nocapture`, so a family that does not compile at an old commit costs only
itself. The output directory holds, per build (`default`, `bundled`, and
`lines` for the line probes, which have no `bundled-fonts` build):

- `raw/<family>.txt`, cargo's output and its exit status;
- `<family>.txt`, the same normalised: the data lines sorted, test results
  without their times, failing tests, panics, compiler errors and the exit
  status, and nothing else;
- `<family>.exit`, and `done` once every family of that build has run, naming
  the ones whose exit was not 0;
- `tree.txt` with the checkout's HEAD and status before and after, and `done`
  at the top once everything asked for is finished.

A run without its `done` marker did not finish, and a family whose rows fall
short of the baseline's was cut off; neither is a measurement (`table.py`'s
`miss` column counts the books missing).

## Reading the tables

`table.py` reads the baselines and, given one, a run directory:

```bash
# 9297d2d -> 56dfd93 -> 935b077, every family, both builds:
python3 -I tools/probes/rtl/scripts/table.py
# ... -> a new run, by group, listing up to five books of each kind:
python3 -I tools/probes/rtl/scripts/table.py /tmp/rtl/now --by group --show 5
```

Per family (or group) and build it prints the books now, those missing
against 935b077, the books conserving at each tree, and:

- `>B`, books worse than at 9297d2d in the strict sense the round reports use:
  conserving lost, or read whole, found by search or `holds` lost;
- `>M`, books conserving at 56dfd93 and not now;
- with a run, `>T` as books worse than at 935b077 in any metric / in
  conserving, and `+T` as books conserving now and not at 935b077;
- `over-ws`, books drawing a glyph over another under word spacing or
  justification, at each tree;
- notes: books with a `VOVX` overprint, `probe_paren`'s failing books,
  fixtures whose hashes changed, `ZZN` overprint, `LINE` lines gone, and any
  exit status that was not 0.

`--show N` lists up to N books of each of `>B`, `>M` and `>T` with their
metrics at each tree (`c` conserving, `t` read whole, `s` found, `h` holds; 1
true, 0 false). `--json FILE` writes every such book, which `classes.py FILE`
sorts into ROADMAP CD-19's classes.

The other comparators:

- `cmp.py A B` compares two outputs book by book, A to B: two run or baseline
  directories (or a baseline's name), or two family files. It prints the books
  each side lacks, those that lost or gained a metric, and overprint that rose.
- `tsvdiff.py A B` compares the line probes' TSVs (and the td probe's): the
  lines `OK` in A and `NO` in B, and the reverse.
- `price.py [RUN]` states ruling 14's price per opening of lane R's `price/`
  and `soft/` books against 56dfd93 and 9297d2d, as the round reports state it.

A probe reruns one book or a few with `RV_FILTER` (the rr3m families, a
substring of `<style>|<name>`) or `RV3_FILTER` (the `k11rv3` ones), and prints
long texts in full with `RV_FULL=1`; run it with cargo directly, since the
runner unsets them. To look at a book's drawn lines and content stream,
`scripts/diag.sh` runs `probes/diag/zz_fx2dbg.rs` over a TSV of books
(`name`, `lang`, `style`, `body`); `probes/diag/open-cases.tsv` holds a few the
second CD-19 fix left open:

```bash
tools/probes/rtl/scripts/diag.sh . default tools/probes/rtl/probes/diag/open-cases.tsv /tmp/rtl/diag.txt lines cs
```

## Regenerating a baseline

A baseline is a run at a commit, packed. With a worktree beside this one:

```bash
git worktree add --detach ../rtl-at-56dfd93 56dfd93
tools/probes/rtl/scripts/run.sh ../rtl-at-56dfd93 both /tmp/rtl/56dfd93
python3 -I tools/probes/rtl/scripts/pack.py /tmp/rtl/56dfd93 /tmp/rtl/56dfd93-packed \
  --note '{family} at 56dfd93, {build}: run with scripts/run.sh'
git worktree remove ../rtl-at-56dfd93
```

`pack.py` refuses a run without its `done` marker and writes every normalised
output and TSV with `xz -9` in the layout of `baselines/`. Compare the packed
directory with the committed one (`cmp.py`, `tsvdiff.py`) before replacing
anything: a baseline that moves when nothing under it changed is a finding,
not a refresh. A family that calls an API the commit does not have fails to
compile there, and its output says so (`zz_fv_shape` and `zz_code1_time` at
9297d2d, which has no `bidi::drawn_reading`).

## Counted injections

`inject.py` puts a rule back broken, runs the suites that should notice, counts
the tests that fire, and restores the source, checking its sha256 after every
run, interrupted or not. Its catalogue, [injections.json](scripts/injections.json),
holds the injections of the K-11 and CD-19 rounds whose text still occurs at
935b077: 58, of which the second CD-19 fix's 14 carry the counts its commit
message records (default build, `bundled-fonts` in brackets); the earlier
rounds' counts are in their fixes' commit messages. Fourteen more targeted code
that has changed since and were left out, and so was one whose round edited a
second line in code of its own (the code review's `bx-full-tw-paint` is the
same injection, whole). An injection that several rounds ran is kept once,
under the latest, with the others named in `also`.

```bash
python3 -I tools/probes/rtl/scripts/inject.py list
python3 -I tools/probes/rtl/scripts/inject.py check .
python3 -I tools/probes/rtl/scripts/inject.py run . /tmp/rtl/inj --build bundled control readings-off no-pairs
```

A count from a run whose suites did not all report a `test result`, or in
which a crate did not compile, is absent, not zero (CONTRIBUTING.md, "Working
an item"). At 935b077, `run . OUT control readings-off no-pieces` gave 0, 2
and 0 in the default build, the counts recorded.

## Where the baselines came from

`baselines/<commit>/<build>/<family>.txt.xz`, and the TSVs beside them, are the
rounds' own outputs, normalised by `rtlkit.normalise` and compressed. Each file
opens with a `#` line saying what it is.

- **935b077**: the second CD-19 fix's last full run of every family, both
  builds, on 7f1b3f8, whose code is 935b077's: `paint.rs` differs in doc
  comments only, and `bidi.rs`, `text.rs`, `text_order.rs` and `build.rs` are
  byte for byte the same.
- **9297d2d** and **56dfd93**: the rounds' runs of the same probe files at
  those trees. For every family the books at both are the books at 935b077.
- The `zz_vx_*` families at 9297d2d and 56dfd93 are the lines of the families
  they wrap, which ran there; their `VOVX` lines were not measured at those
  trees, and those files say so.
- Measured for this kit on 12 October 2026 with `run.sh`: `zz_code1_neg` and
  `zz_code1_tdprobe` at 9297d2d, `zz_code1_tdprobe` at 935b077, and the
  regressions' outcome at 935b077, each in both builds.
- Not measured: `zz_code1_neg` and `zz_code1_tdprobe` with `bundled-fonts` at
  56dfd93, and the timing families anywhere. `zz_fv_shape` does not compile at
  9297d2d, and its baseline there is the compiler's error.

Rerunning `zz_rr3m_hunt_min` at 9297d2d, and `zz_rr3m_hunt_min`,
`zz_rr3m_h1min2`, `zz_c1_tw`, `zz_code1_neg` and every line probe at 935b077,
with `run.sh`, reproduced the baselines' data lines and TSVs exactly. Read with
`table.py`, `price.py` and `classes.py`, the baselines give the figures the
second CD-19 fix's commit message reports, in both builds: the books
conserving at 9297d2d, 56dfd93 and now, those worse than either baseline, the
4 342 (3 622) books of ruling 14's price, and the Left list's classes.

## The regressions

Each file in `probes/regressions/` is a verifier's minimised repro of the
findings of one review, one test a book or two. A test asserts what its
finding said a book did at the tree the verifier ran it on, which was a
baseline or the fix under review, so a test that fails at 935b077 is not by
itself a regression: it may be a finding fixed since, a finding still open
(ROADMAP CD-19, Left), or ruling 14's price. The runner runs them as the group
`regressions`; their outcome at 935b077 is under `baselines/935b077/<build>/`
beside the families', and `table.py` prints it under each build's table:

| File | Review | Tests | Pass at 935b077, default (bundled-fonts) |
| --- | --- | ---: | --- |
| `zz_vc_verify` | K-11 round 4, code | 4 | 3 (3) |
| `zz_vc_pal` | K-11 round 4, confirm | 2 | 2 (2) |
| `zz_vh_verify` | K-11 round 4, hunt | 5 | 5 (5) |
| `zz_vc1_repro` | round 5, code | 13 | 7 (8) |
| `zz_vc1_min` | round 5, confirm | 21 | 14 (13) |
| `zz_vfv1` | the first CD-19 fix, verification | 11 | 5 (7) |
| `zz_vh1` | round 5, hunt | 5 | 2 (2) |
| `zz_vh2t` | the first CD-19 fix, hunt | 16 | 10 (11) |

Which of them become guards of what the tree does now, and how the ones for
open findings are held, is the conversion step's to settle, finding by
finding.

## What comes next

AGENTS.md asks that a probe family that found something become a committed
suite: an `#[ignore]`d integration test that prints a `RAN` banner and runs on
demand or nightly, with its baseline committed. These families are the input
to that step. Each becomes a test file under `crates/tinker-pdf/tests/` (or
`crates/tinker-pdf-shape/tests/`), its books unchanged and its output compared
with the baseline here, and `run.sh` shrinks to `cargo test -- --ignored`. The
regressions are read against their findings first, as above.
