# Right-to-left probe kit

The probes the K-11 and CD-19 rounds wrote to measure right-to-left text in
EPUB books, the scripts that run and compare them, and their outputs at three
trees. They lived in one machine's scratch directory until now; they are
committed so that the next round starts from the repository and not from a
directory a container restart can lose ([AGENTS.md](../../../AGENTS.md), "A
test for every case").

The families are committed tests as well: two `#[ignore]`d integration-test
binaries, `crates/tinker-pdf/tests/rtl_probes.rs` and
`crates/tinker-pdf-shape/tests/rtl_lines.rs`, build every family's books and
fail when one reads worse than at `935b077`, each family's test printing a
`RAN` banner ([The suites](#the-suites)). That is how a change is held to the
kit.

Nothing in this directory is compiled by cargo. The `.rs` files here are the
probes as they ran; they sit outside every crate's `tests/` directory and
`tools/probes` is not a workspace member, so no build, test, clippy or fmt run
sees them. The runner copies a probe into a checkout's `crates/<crate>/tests/`,
runs it there, and removes exactly what it copied, which is how the three
trees were measured and how another tree is.

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
logical=true|false [search=...]`. Beside `ROW` the families print the lines
below, and `rtlkit.Output` reads every one of them, so `cmp.py` and `table.py`
see a change in any:

| Line | Family | Says |
| --- | --- | --- |
| `VOV` | the rr3m families | per book that has any: how many visible glyphs are drawn over another on one baseline (the `b` of its `overlap=a/b`) and where the first one is; a book with no `VOV` line has none |
| `VOVX` | `zz_vx_*` | per book (style, language, hash of the body): visible glyphs drawn over another |
| `BOOK` | `zz_vx_k11rv_dirless` | whether each dir-less book's lines hold (`holds=`), with a `LINE` per misread line |
| `TALLY` | `zz_vx_k11rv_dirless` | per gloss and kind of line (`right`, or which way it was misread), how many lines; the comparators count the lines misread, since a change of line breaking changes how many lines there are |
| `FAIL`, `rows=` | `zz_vx_probe_paren` | the books whose bracketed run and full stop do not conserve, and the total |
| `FIX` | `zz_k11rv_fixtures` | every local fixture's text in both orders and its content streams, hashed |
| `TWROW` | `zz_c1_tw` | strings the overflow font shows, those holding code 32 (`bx32`), those shown at a word spacing other than zero (`twbad`), overprint (`vov`), `cons`, `logical` |
| `ZZN` | `zz_code1_neg` | glyphs drawn over another under a negative word spacing |
| `ZZ rows` | `zz_code1_tdprobe` | how many rows it wrote to its TSV |
| `PANIC` | the rr3m families | a book whose probe panicked, beside a `ROW` of zeros for it |
| `LINE` | `zz_fv_shape` | one line's text and its reading |
| `SENT`, `WRAPPED`, `BCT-AUTO`, `LANER` | the line probes | their own totals; the lines themselves are in their TSVs |

A `VOV` glyph is drawn over another where its box and another's overlap on one
baseline, so under `transform: skewX` a slanted glyph's box reaches over its
neighbour's where nothing is drawn over anything (V7 of round 6's review):
every book of round 6's hunt that the count finds so under word spacing or
justification at 935b077, 80 in each build of `zz_v6_transform`, is under
`skewX(-15deg)`.

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
| `v6` | eight `zz_v6_*` | 8 472 | nested inline styles, lists and generated content, spacing units and the `0.45em` and `0.5em` thresholds, supplementary-plane scripts, numbers with every separator, table cells, measures, `dir=auto` and `<bdi>` | round 6's hunt (lane V) |
| `v6t` | `zz_v6_transform` | 1 120 | CSS transforms and tight line heights under spacing | round 6's hunt (lane V) |
| `f6` | `zz_f6_transform`, `zz_f6_ems` | 3 654 + 264 | the class of the hunt's V1: a scale of one axis or both, turns, skews, a matrix and a shift, on the paragraph, its block, both and an inline block, under spacing and none; and the edges of its fix: two cells whose gap is near half an em, inline blocks of about half an em, marks of no advance opening a styled run, under no transform, a scale of one axis and a skew | round 6's fix (lane F) |
| `f6inj` | `zz_f6_readas`, `zz_f6_unfired` | 464 + 170 | characters a page's fonts read otherwise than they were typed, so that reading a line back as the fonts give it back (`read_as`) can decide its drawing: a NO-BREAK SPACE in a number, before a sign, a comma or a styled `?!`, a SOFT HYPHEN, letters past the Basic Multilingual Plane, under spacing and a scale of one axis; and books for the rules of reading a line back that a build's suites did not fire when each was put back broken: dir-less lines under a scale of one axis or a matrix (`reads_whole`, with `bundled-fonts`), squeezed inline blocks, styled marks and signs turned (`reads_closer`, with `bundled-fonts`), a number with a NO-BREAK SPACE before an ARABIC COMMA (`read_as`, in both) | round 6's counted injections (lane F) |
| `v6pdf` | `zz_v6_pdf` | 540 | plain synthetic PDFs drawn as producers draw right-to-left lines, read by ruling 14 (no EPUB) | round 6's hunt (lane V) |
| `f6pdf` | `zz_f6_h23price` | 64 | plain synthetic PDFs: two words of one script, each its own text object, 0.3em to 2em apart with no space glyph, as two positioned words and as two table cells, in Greek, Cyrillic, Armenian and Latin; what answering H2-3 in the reader would cost | round 6's owner options (lane F) |
| `v6time` | `zz_v6_time` | | what reading each line back costs `Document::open`, at doubling sizes | round 6's hunt (lane V) |

Each `zz_vx_*` family is an earlier family with a shim around
`styled_book` that prints a `VOVX` line a book: `zz_vx_k11rv_ext` is
`zz_k11rv_ext` and so on, and `zz_vx_rv4code` is the round-4 code review's
`zz_rv4code`. At 935b077 their `ROW`, `BOOK` and `FAIL` lines are byte for
byte those of the families they wrap, in both builds, so the originals are not
kept.

[probes/diag/](probes/diag/) holds one diagnostic probe. The verifiers'
minimised tests are committed tests now
([The regressions](#the-regressions)). The probes are committed as they ran:
their opening comments still call them scratch, which is what they were when
they produced the baselines. One has changed since, in what it prints on
Windows only ([On Windows](#on-windows)), and round 6's timing probe,
`zz_v6_time`, times two shapes its hunt did not, `ls-scaled` and
`rtl-justified-scaled`, added while round 6's fix was measured; a timing has
no baseline.

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

## The suites

Every family a ratchet reads is also a committed test, in two `#[ignore]`d
integration-test binaries, so that CI compiles two binaries and not seventy:

- `crates/tinker-pdf/tests/rtl_probes.rs`: one test a `rows` or `tsv` family
  of [families.tsv](families.tsv), 78 in all, each its module under
  `rtl_probes/`; the harness the rr3m families share is
  `rtl_probes/common.rs`, the overprint shim of the `zz_vx_*` families
  `rtl_probes/vov.rs`, and the comparison `rtl_probes/ratchet.rs`;
- `crates/tinker-pdf-shape/tests/rtl_lines.rs`: one test a `lines` family, 4
  in all, under `rtl_lines/`.

```bash
cargo test -p tinker-pdf --test rtl_probes -- --ignored --nocapture
cargo test -p tinker-pdf --features bundled-fonts --test rtl_probes -- --ignored --nocapture
cargo test -p tinker-pdf-shape --test rtl_lines -- --ignored --nocapture
# one family, or a few:
cargo test -p tinker-pdf --test rtl_probes zz_rr3m_hunt_min -- --ignored --nocapture
```

Each test builds its family's books exactly as the probe here does (the
modules are the probes with their shared code taken out, their `println!` sent
to the ratchet, and the knobs that narrowed a run removed) and prints one line:

```text
RAN rtl_probes zz_rr3m_hunt_min [default] against 935b077: 12 rows (the baseline has 12), 0 books worse, 0 books better, 0 changed without a verdict
RAN rtl_lines zz_k11rv_bidi against 935b077: 93851 rows, 2 printed lines; 0 worse, 0 gains, 0 changed without a verdict
```

then a `WORSE`, `GAIN` or `CHANGED` line for each book that moved (up to 40 of
each), and fails if any book reads worse than at 935b077 in that build. A run
of the three commands prints 78, 78 and 4 `RAN` lines; fewer means a family
did not run. Worse is any of:

- a book the baseline has and the run lacks, or fewer rows;
- `content`, `logical`, `cons`, read whole (`text`), found by search
  (`search=a/b`, `a == b`) or `holds`, true at 935b077 and not now;
- a glyph drawn over another more often (the rr3m `overlap`, `VOV`, `VOVX`,
  `TWROW`'s `vov`, `ZZN`), or the overflow font showing code 32 at a word
  spacing other than zero more often (`TWROW`'s `twbad`);
- a `FAIL` key `zz_vx_probe_paren` did not have, more dir-less lines misread
  (`TALLY`) or more misread `LINE`s in a book, a fixture whose hashes moved
  (`FIX`), a book whose probe panicked, the td probe writing fewer rows;
- for a line family, a key with more rows `NO`, or `undrawn`, than at 935b077.

A gain is the same metrics the other way; it is printed and never fails.
Lines that carry no verdict (the td probe's rows, `zz_fv_shape`'s readings, a
book's `thash` and `chash`) are printed when they change, as `CHANGED`, for a
person to read, and never fail. Timing families stay probes: a wall-clock time
is no measurement another machine repeats.

The suites read `baselines/935b077/` with this repository's own code (ruling
13): `rtl_probes` decodes the `.xz` files through `tinker-pdf-archive`'s LZMA2
decoder, checking every header's CRC-32 and each block's CRC-64, and
`rtl_lines`, whose crate has no decompressor and takes none for a test, reads
the plain `lines/*.txt` and `lines/*.not-ok.tsv` (each line TSV reduced to its
row count and the rows not read right or not drawn, `rtlkit.not_ok`).

At 935b077's code every family passes in both builds with nothing worse,
nothing better and nothing changed, and its data lines and TSVs are the
baseline's byte for byte (the `TINKER_RTL_KIT_OUT` directory below against the
decoded baselines; `cmp.py` and `tsvdiff.py` read it as 0 worse, 0 better, 0
changed): on Linux for the 63 families before round 6, and for all 78 on the
owner's Windows machine (Windows 11, 8 logical CPUs) at a tree whose
`crates/*/src` is 935b077's. In debug the 63 tinker-pdf families took 13
minutes in a default build and 15 with `bundled-fonts` on four cores on Linux,
and the 4 line families 10 seconds; on the Windows machine the 78 take
6 minutes in a default build and 7 with `bundled-fonts`, and the line families
6 seconds (369, 431 and 6 seconds of the tests' own time).

At round 6's fix, which reads each line back on the page it is drawn on
(ROADMAP CD-19), every family passes in both builds with nothing worse, 361
books better in a default build and 415 with `bundled-fonts`, and on the
Windows machine the suites take 6 and 7 minutes and 6 seconds (365, 398 and
6 seconds of the tests' own time).

Both binaries also hold their comparison to this section in tests that are not
ignored and build no book (`cargo test -p tinker-pdf --test rtl_probes`, `cargo
test -p tinker-pdf-shape --test rtl_lines`, which every `cargo test` runs): each
kind of worse above, the gains, the changes that carry no verdict, the
normaliser, the decoder refusing a damaged file, a block whose CRC-64 does
not hold and a check it does not compute, and `zz_k11rv_fixtures`'s names,
the baseline's on every platform ([On Windows](#on-windows)). Each rule of
the two comparisons is an injection in
[injections.json](scripts/injections.json), named `suites/…`.

The suites hold the families' books by the thousand against a baseline. The
books a review named one by one are held otherwise: each is a case, a row of
the table-driven test of its mechanism in
`crates/tinker-pdf/tests/epub_conservation.rs` and `epub_fallback.rs`, read
as this kit reads a book (`tests/epub_support/reading.rs`, whose text, words
and marks `rtl_probes/common.rs` takes too) and held to its right reading or,
while its finding is open, pinned at today's. Those tests are not ignored,
build their books in seconds and run with every `cargo test`; their rules are
the injections named `cases/…`. [The regressions](#the-regressions) says which
of them carries which of the verifiers' tests.

### The suites and the runner

The probes here are the families as they ran at the three trees, and `run.sh`
runs them at any checkout, an old tree included, where the suites' baseline
does not apply. The suites run the same books at this checkout and hold them
to 935b077. A suite is where a family changes from now on: a book added to a
suite module is not in its probe, and the probe stays as it ran.

With `TINKER_RTL_KIT_OUT=DIR` set, each suite also writes what it printed in
the layout of a kit run: `DIR/<build>/<family>.txt` (the data lines, sorted, and
`exit 0` once its books are all built) and its TSVs beside it, and the line
suite `DIR/lines/`. The comparators read that directory as they read a
`run.sh` output: `table.py DIR`, `cmp.py 935b077 DIR`, `tsvdiff.py 935b077 DIR`.
Setting it changes nothing a suite asserts.

### Refreshing the baseline

The baseline moves only for a reason a person can name: a gain worth keeping
as the new floor, or a price the owner accepted (AGENTS.md, "The owner decides
prices"; ruling 14 records the price and its figures). A baseline that moves
when nothing under it changed is a finding, not a refresh. To refresh:

1. Run the three suite commands with `TINKER_RTL_KIT_OUT=DIR` at the commit
   whose readings become the floor, and check every `RAN` line and every exit
   status. A run with a family worse than the old baseline fails; that is
   expected for an accepted price, and every `WORSE` line must be one the
   ruling names.
2. Compare: `python3 -I tools/probes/rtl/scripts/cmp.py 935b077 DIR --show 20`
   and `tsvdiff.py 935b077 DIR`, and read every book that moved.
3. Pack: `python3 -I tools/probes/rtl/scripts/pack.py DIR
   tools/probes/rtl/baselines/<commit> --suites --note '{family} at <commit>,
   {build}: the suites'' run'`. `--suites` refuses a directory in which a family
   did not finish.
4. Point `BASELINE` in both `ratchet.rs` files at `<commit>`, rerun the suites
   (every family passes with nothing worse), and say in the commit message what
   moved, by how many books, and on whose decision.

The old baseline directory stays: the three-tree tables (`table.py`) read
9297d2d, 56dfd93 and 935b077 by name.

## Running the probes

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
information off, and deletes each probe's test binary after its run
(`KEEP_BINARIES=1` keeps them). It unsets every variable a probe reads with
`env::var` (`RV_FILTER`, `RV3_FILTER`, `RV4C_FILTER`, `RV_FULL` and the
diagnostic probe's `DBG_` knobs), finding them by reading the probes rather
than from a list, except the two it sets itself, `K11RV_OUT` and `ZZ_OUT`. The
run that produced the 935b077 baseline took about an hour a build.

An interrupt stops the whole run and writes no done marker: Ctrl-C, `SIGINT`,
`SIGTERM` or `SIGHUP` sent to the runner, and a family whose cargo or test
binary was killed from outside (by one of those or `SIGKILL`). Each cargo runs
as a job in its own process group, so a signal reaches the runner first and
the runner passes it on to the whole job; `progress` says where the run
stopped. A shell that starts the runner in the background with `SIGINT`
ignored cannot hand it back, and the runner says so when it starts: stop such
a run with `kill -TERM`.

Each family runs on its own: one `cargo test -p <crate> --test <family> --
--nocapture`, so a family that does not compile at an old commit costs only
itself. The output directory holds, per build (`default`, `bundled`, and
`lines` for the line probes, which have no `bundled-fonts` build):

- `raw/<family>.txt`, cargo's output and its exit status;
- `<family>.txt`, the same normalised: the `Running tests/<family>.rs`
  header, the data lines sorted, test results without their times, failing
  tests, panics, compiler errors and the exit status, and nothing else, the
  header's path and a panic's file with a `/` on every platform;
- the TSVs the probes write, exactly as they write them: the line probes'
  `lines/bct-auto.tsv`, `wrapped.tsv`, `sent.tsv` and `laneR.tsv`, and the td
  probe's `<build>/zz_code1_tdprobe.tsv`;
- `<family>.exit`, and `done` once every family of that build has run, naming
  the ones whose exit was not 0;
- `progress`, one line a family as it finishes (build, family, exit status,
  data lines, seconds), and a line saying where an interrupted run stopped;
- `tree.txt` with the checkout's HEAD and status before and after, and `done`
  at the top once everything asked for is finished.

`lines/` is made only when a line probe runs. A run into a directory that
already holds one removes the done markers it could write before it starts,
so an earlier run's markers never vouch for this one.

A run without its `done` marker did not finish, and a family whose rows fall
short of the baseline's was cut off; neither is a measurement (`table.py`'s
`miss` column counts the books missing). The comparators say so: `cmp.py`,
`table.py`, `tsvdiff.py` and `price.py` print an `INCOMPLETE` line at the
end for each build run.sh started and wrote no done marker for, and for the
families (TSVs) one side has and the other lacks, and exit 1; a whole
comparison prints what it always did. A baseline and a `TINKER_RTL_KIT_OUT`
directory have no marker: `pack.py` packs no unfinished run, and a suite
writes a family's output only once its books are all built.

### On Windows

The kit runs under Git for Windows' `bash` with a native Python in UTF-8 mode
(`python -X utf8`: `-I` ignores `PYTHONUTF8`, and the scripts print Hebrew and
Arabic through pipes). A run's normalised outputs and TSVs, and the suites'
dumps, are then the ones Linux writes, byte for byte. Until 17 October 2026
two of the kit's readings depended on the platform's path separator, and
both read alike on every platform now:

- `zz_k11rv_fixtures` named a fixture by its path's text with the root's
  taken out, so on Windows every name was `\crates\...` and the suites read
  all 107 fixtures as missing; its probe and its suite module both build the
  name from the path's components, `/crates/...` everywhere, and read a seed
  of the fuzz corpus by that name.
- cargo on Windows heads a test binary's output `Running tests\<family>.rs`,
  which the normaliser did not read: it dropped the header, and with
  `--binary` it kept nothing but the exit status. It reads either form and
  writes `Running tests/<family>.rs`. A panic it keeps names its file with
  a `/` too, where Windows prints `panicked at crates\...`.

The suites' own tests hold the names, and `python3 -I
tools/probes/rtl/scripts/selfcheck.py` holds the normaliser on both forms,
and `inject.py`'s reading of what cargo prints on Windows.
Three things still differ on Windows:

- A test binary killed from outside comes back from cargo as an exit code
  (`exit code: 1` after `taskkill /F`, `0xffffffff` after `Stop-Process`),
  not a signal, so `run.sh` reads it as a family that failed rather than an
  interrupt: the run goes on, and its done marker names the family among
  those not exit 0.
- A probe run by cargo directly, outside `run.sh`, writes its TSV where its
  variable is unset to `/tmp` or `/dev/null`, which Windows reads as
  `\tmp` and `\dev\null` on the current drive: `zz_code1_tdprobe` panics
  after printing its rows, and a line probe does unless `\tmp` exists.
- The scripts' reports on stdout, and `inject.py`'s summary and logs, end
  their lines with CRLF (Python's text mode). Nothing in the kit reads them
  back but `run.sh`, which reads `norm.py`'s count through Git Bash's
  `$(...)`, and that drops the CR.

## Reading the tables

`table.py` reads the baselines and, given one, a run directory:

```bash
# 9297d2d -> 56dfd93 -> 935b077, every family, both builds:
python3 -I tools/probes/rtl/scripts/table.py
# ... -> a new run, by group, listing up to five books of each kind:
python3 -I tools/probes/rtl/scripts/table.py /tmp/rtl/now --by group --show 5
```

Per family (or group) and build it prints the books now, those missing (with
a run, 935b077's books the run lacks, the sign of a truncated run; without
one, 9297d2d's books 935b077 lacks), the books conserving at each tree, and:

- `>B`, books worse than at 9297d2d in the strict sense the round reports use:
  conserving lost, or read whole, found by search or `holds` lost;
- `>M`, books conserving at 56dfd93 and not now;
- with a run, `>T` as books worse than at 935b077 in any metric / in
  conserving, and `+T` as books conserving now and not at 935b077;
- `over-ws`, books drawing a glyph over another under word spacing or
  justification, at each tree;
- notes: books with a `VOVX` overprint, books with a `VOV` line (and, with a
  run, those whose overprint rose), `probe_paren`'s failing books, fixtures
  whose hashes changed, `ZZN` overprint, the td probe's `ZZ` row count, the
  dir-less lines `TALLY` counts misread, books whose probe panicked, `LINE`
  lines gone, and any exit status that was not 0.

With a run, a family the run has no output for is left out, not read as a
family that lost every book: a run of one group compared `--by group` shows
that group, and one line under each build's table names the families not run.
`cmp.py` does the same, naming the families only one side has. Either is then
incomplete, and exits 1 (`--families` narrows what both sides must have).

`--show N` lists up to N books of each of `>B`, `>M` and `>T` with their
metrics at each tree (`c` conserving, `t` read whole, `s` found, `h` holds; 1
true, 0 false). `--json FILE` writes every such book, which `classes.py FILE`
sorts into ROADMAP CD-19's classes.

The other comparators:

- `cmp.py A B` compares two outputs book by book, A to B: two run or baseline
  directories (or a baseline's name), or two family files. Per family and
  build it prints the rows on each side, how many books A has and B lacks
  (`missing`), the books that lost or gained a metric, and overprint that
  rose; `--show N` lists up to N of the books that lost, gained or rose. The
  missing books are counted, never named, and a book only B has shows only in
  B's row count: A with 99 rows against B with 213 prints `rows 99/213
  missing 0`. Run it both ways round to count each side's.
- `tsvdiff.py A B` compares the line probes' TSVs: the lines `OK` in A and not
  in B (`worse`), and the reverse (`better`), listing up to `--show N` of each
  (20 by default). The td probe's TSV carries no verdict, so its rows are
  compared whole and the ones that differ are counted and listed as `changed`.
  As in `cmp.py`, `missing` counts the rows A has and B lacks without naming
  them.
- `price.py [RUN]` states ruling 14's price per opening of lane R's `price/`
  and `soft/` books against 56dfd93 and 9297d2d, as the round reports state it.

A probe reruns one book or a few with `RV_FILTER` (the rr3m families, a
substring of `<style>|<name>`), `RV3_FILTER` (the `k11rv3` ones) or
`RV4C_FILTER` (`zz_vx_rv4code`, a substring of `s<style>\t<name>\t<label>`),
and prints long texts in full with `RV_FULL=1`; run it with cargo directly,
since the runner unsets them. To look at a book's drawn lines and content stream,
`scripts/diag.sh` runs `probes/diag/zz_fx2dbg.rs` over a TSV of books
(`name`, `lang`, `style`, `body`); `probes/diag/open-cases.tsv` holds a few the
second CD-19 fix left open:

```bash
tools/probes/rtl/scripts/diag.sh . default tools/probes/rtl/probes/diag/open-cases.tsv /tmp/rtl/diag.txt lines cs
```

## Regenerating a baseline

This is how the three trees' baselines were made, and how another tree's is:
a `run.sh` run at a commit, packed. The suites' floor is refreshed from the
suites instead ([Refreshing the baseline](#refreshing-the-baseline)). With a
worktree beside this one:

```bash
git worktree add --detach ../rtl-at-56dfd93 56dfd93
tools/probes/rtl/scripts/run.sh ../rtl-at-56dfd93 both /tmp/rtl/56dfd93
python3 -I tools/probes/rtl/scripts/pack.py /tmp/rtl/56dfd93 /tmp/rtl/56dfd93-packed \
  --note '{family} at 56dfd93, {build}: run with scripts/run.sh'
git worktree remove ../rtl-at-56dfd93
```

`pack.py` refuses a run without its `done` marker and writes every normalised
output and TSV with `xz -9` in the layout of `baselines/`, except the line
families' outputs, which it writes as plain text, and beside each line TSV its
`.not-ok.tsv` (both below). `--note` puts a `#` line at the top of each
normalised output; the TSVs are copied as the probes wrote them, with no
note. Compare the packed directory with the committed one
(`cmp.py`, `tsvdiff.py`) before replacing anything: a baseline that moves when nothing under it changed is a finding,
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
under the latest, with the others named in `also`. The 20 named `suites/…`
put back broken each rule of the two suites' comparisons, and fire the
suites' own checks (round 6's commit of the suites records their counts).
Round 6's fix of CD-19, which reads each line back on the page it is drawn
on, added the 22 named `page/…` and rewrote two of 935b077's, `no-pairs` and
`lines-touching`, to the code it changed under them; its commit message
records their counts. Round 6's review fix of what a drawing found in the
runs' own space is compared by added `page/closer-by-pairs-alone` and
`page/closer-ties-unbroken`, and moved `page/closer-unchecked` to its
comparison: 108 injections in all.
`--suite` runs a suite of your choosing instead of the catalogue's, the
suites themselves included:

```bash
python3 -I tools/probes/rtl/scripts/inject.py run . /tmp/rtl/inj \
  --suite 'cargo test --no-fail-fast -p tinker-pdf --test rtl_probes -- --ignored' bx-full-tw-paint
```

```bash
python3 -I tools/probes/rtl/scripts/inject.py list
python3 -I tools/probes/rtl/scripts/inject.py check .
python3 -I tools/probes/rtl/scripts/inject.py run . /tmp/rtl/inj --build bundled control readings-off no-pairs
```

A count comes from the bytes on disk and from every test binary a run
started, or it is absent, not zero (CONTRIBUTING.md, "Working an item").
Cargo calls a crate fresh by file times, so a tree whose files are older than
a target's last build (another tree's injected one, in a shared target) would
run that build: round 6's review saw a control fire 1 and, rebuilt, 0. So
before every run, the control included, `inject.py` touches every file the
call's injections edit, and a run is incomplete unless its log shows each
crate it had to build compiling (its own edits' crates; for the control,
every touched file's). A crate no injection of the call names is not touched,
so give each tree a target of its own. A run is also incomplete when a crate
did not compile, when a test binary cargo started printed no `test result`
(it crashed: a stack overflow, an abort), or when a suite exited non-zero
naming no failing test; cargo's output and its binaries' are read as one
stream, in the order printed. At 935b077, `run . OUT control readings-off
no-pieces` gave 0, 2 and 0 in the default build, the counts recorded.

## Where the baselines came from

`baselines/<commit>/<build>/<family>.txt.xz` are the rounds' own outputs,
normalised by `rtlkit.normalise` and compressed, and each opens with a `#` line
saying what it is. The line families' are the same outputs uncompressed,
`lines/<family>.txt`, so that `rtl_lines` reads them with no decompressor
(ruling 13); they are a few hundred bytes each. The 17 TSVs beside them are
not normalised and carry no `#` line. Each `.tsv.xz` is a probe's TSV as the
probe wrote it, compressed and nothing else: `lines/bct-auto`, `wrapped`,
`sent` and `laneR` at each tree, and `zz_code1_tdprobe` in both builds at
9297d2d and 935b077 and in the default build at 56dfd93. Rerun at 935b077 with
`run.sh`, the line probes and the td probe, in both builds, write theirs byte
for byte, and so do the suites. `935b077/lines/<tsv>.not-ok.tsv` is each line
TSV reduced by `rtlkit.not_ok` to a `#` line, its row count and the rows not
read right or not drawn, by key and verdicts: what `rtl_lines` holds a run
to.

- **935b077**: the second CD-19 fix's last full run of every family, both
  builds, on 7f1b3f8, whose `crates/` tree is 935b077's: the two commits
  differ only in `.claude/`, `.gitignore`, `AGENTS.md` and `CLAUDE.md`
  (`git diff --stat 7f1b3f8 935b077`). This file and the baselines' note lines
  said until 16 October 2026 that `paint.rs` differed in doc comments, which
  was never true.
- **9297d2d** and **56dfd93**: the rounds' runs of the same probe files at
  those trees. For every family the books at both are the books at 935b077.
- The `zz_vx_*` families at 9297d2d and 56dfd93 are the lines of the families
  they wrap, which ran there; their `VOVX` lines were not measured at those
  trees, and those files say so.
- Measured for this kit on 12 October 2026 with `run.sh`: `zz_code1_neg` and
  `zz_code1_tdprobe` at 9297d2d and `zz_code1_tdprobe` at 935b077, each in
  both builds.
- Not measured: `zz_code1_neg` and `zz_code1_tdprobe` with `bundled-fonts` at
  56dfd93, and the timing families anywhere. `zz_fv_shape` does not compile at
  9297d2d, and its baseline there is the compiler's error.
- **Round 6's hunt and sweep families** (`zz_v6_*`, `zz_f6_transform` and
  `zz_f6_ems`, 14 050 books a build) were made on Linux in the cloud on 16
  October 2026: at 9297d2d and 56dfd93 by `run.sh`, lane V's runs of the
  hunt's families and plain PDFs and lane F's of the sweep, packed with
  `pack.py`; at 935b077 by `run.sh` at 05cea0b, and for `zz_v6_pdf` at
  d1c32cd, both of whose `crates/*/src` are 935b077's. Each file's `#` line
  says where it ran. The owner's Windows machine reproduced them on 17
  October 2026: `run.sh` there wrote the twelve families' data lines at
  9297d2d and 56dfd93 byte for byte, and the suites, at a tree whose
  `crates/*/src` is 935b077's, read all 75 families' 935b077 outputs with
  nothing worse, better or changed, their dump the baselines' data lines and
  TSVs byte for byte.
- **`zz_f6_readas`, `zz_f6_unfired` and `zz_f6_h23price`** were made on the
  owner's Windows machine on 17 October 2026, by `run.sh` at each tree in both
  builds, at 935b077 on a checkout of 05cea0b, from scratch probes of round
  6's fix (its counted injections and an option for the owner); at 935b077's
  code they read, book for book, what those probes read: every verdict, the
  overprint and the text. At a tree whose `crates/*/src` is 935b077's the
  suites read their 935b077 outputs with nothing worse, better or changed,
  their dump the baselines' data lines byte for byte.

Rerunning `zz_rr3m_hunt_min` at 9297d2d, and `zz_rr3m_hunt_min`,
`zz_rr3m_h1min2`, `zz_c1_tw`, `zz_code1_neg` and every line probe at 935b077,
with `run.sh`, reproduced the baselines' data lines and TSVs exactly. Read with
`table.py`, `price.py` and `classes.py`, the baselines give the figures the
second CD-19 fix's commit message reports, in both builds: the books
conserving at 9297d2d, 56dfd93 and now, those worse than either baseline, the
4 342 (3 622) books of ruling 14's price, and the Left list's classes.

## The regressions

The verifiers of the K-11 round-4 review and of round 5 each wrote a file of
minimised repro tests, 77 in eight files, asserting what a finding said a book
did at the tree they ran on. They were here, in `probes/regressions/`, as the
group `regressions`, until round 6 carried every one of their books into a
committed test of its mechanism, beside every other book a finding of those
reviews named (`crates/tinker-pdf/tests/epub_support/reading.rs` says how a
case is read and held): a case fixed is held to its right reading, a case
still open is pinned at today's and names the ROADMAP CD-19 item that leaves
it. The files, their group and their outcome at 935b077 went with them; the
round-6 commit that removed them gives each case's reading at the tree before
its fix. Which test carries which:

| Regression test (file) | Carried by (test, `epub_conservation.rs` or `epub_fallback.rs`) |
| --- | --- |
| `zz_vc_verify`: `f1_an_ornate_parenthesis_cut_from_its_line_carries_no_pen`, `f1_control_guillemets` | `a_neutral_the_pen_could_carry_into_a_word_reads_where_it_was_typed` |
| `zz_vc_verify`: `f2_a_line_opening_with_a_bracketed_arabic_number_reads_its_stop_last`, `f2_control_plain_number` | `a_line_opening_with_a_bracket_pair_reads_its_stops_where_they_were_typed` |
| `zz_vc_pal`: `zz_vc_pal_survey`, `zz_vc_pal_witness` | `a_neutral_beside_a_number_or_a_styled_word_is_drawn_for_its_lines_reading` |
| `zz_vh_verify`: `h1_et_after_number_opening_wrapped_arabic_line` | `a_neutral_beside_a_number_or_a_styled_word_is_drawn_for_its_lines_reading` |
| `zz_vh_verify`: `h2_dirless_styled_brackets_full_stop` | `a_line_opening_with_a_bracket_pair_reads_its_stops_where_they_were_typed` |
| `zz_vh_verify`: `h3_styled_qx_glued_letter_spaced_justified`, `h4_word_spaced_overprint_word_found` | `a_styled_neutral_glued_to_a_word_under_spacing_reads_where_it_was_typed` |
| `zz_vh_verify`: `h5_styled_arabic_percent_opening_justified_line` | `a_neutral_the_pen_could_carry_into_a_word_reads_where_it_was_typed` |
| `zz_vc1_repro`: the six `cr1_*` | `a_styled_neutral_glued_to_a_word_under_spacing_reads_where_it_was_typed` |
| `zz_vc1_repro`: the five `cr2_*` | `a_line_opening_with_a_bracket_pair_reads_its_stops_where_they_were_typed` |
| `zz_vc1_repro`: `cr3_table_diag` | `a_cell_or_a_spaced_slice_is_one_text_object_only_where_its_line_reads_it_whole` |
| `zz_vc1_repro`: `cr4_negative_word_spacing_overprint` | `a_standard_14_overflow_letter_at_code_32_takes_no_word_spacing` (already, since ce0b3de) |
| `zz_vc1_min`: `f1_*`, `f2_*`, `f4_*`, the thirteen `f5_*` | `a_styled_neutral_glued_to_a_word_under_spacing_reads_where_it_was_typed` |
| `zz_vc1_min`: `f3_*`, `f6_*` | `a_line_opening_with_a_bracket_pair_reads_its_stops_where_they_were_typed` |
| `zz_vfv1`: `r1_tablecost_read_whole` | `a_cell_or_a_spaced_slice_is_one_text_object_only_where_its_line_reads_it_whole` |
| `zz_vfv1`: `h1_final_stop_conserves`, `h1_monospace_conserves_at_m_only` | `a_left_to_right_hebrew_paragraph_ending_a_number_and_a_stop` |
| `zz_vfv1`: `f1_styled_neutrals_letter_spaced`, `f2_price_he_qe_b2_conserves`, `cr1_vs_m` | `a_styled_neutral_glued_to_a_word_under_spacing_reads_where_it_was_typed` |
| `zz_vfv1`: `h3_vs_m` | `a_neutral_beside_a_number_or_a_styled_word_is_drawn_for_its_lines_reading` |
| `zz_vfv1`: `h4_f3_f4_vs_b` | its `25٪ <b>نعرف</b>` books `a_neutral_beside_…`, its Persian-digit book `a_left_to_right_hebrew_paragraph_…`, F4 `a_styled_neutral_glued_…`, F3 `a_line_opening_with_a_bracket_pair_…` |
| `zz_vfv1`: `h6_mixed_runs_vs_m`, `interaction_logical_vs_m`, `apos_stop_logical_vs_b` | `a_line_opening_with_a_bracket_pair_reads_its_stops_where_they_were_typed` |
| `zz_vh1`: `h1_final_stop_after_a_number` | `a_left_to_right_hebrew_paragraph_ending_a_number_and_a_stop` |
| `zz_vh1`: `h2_overflow_cell_before_rtl_cell` | `a_cell_or_a_spaced_slice_is_one_text_object_only_where_its_line_reads_it_whole` |
| `zz_vh1`: `h3_v3_one_text_object` | `a_neutral_beside_a_number_or_a_styled_word_is_drawn_for_its_lines_reading` |
| `zz_vh1`: `h4_preexisting` | `a_neutral_beside_…` and `a_left_to_right_hebrew_paragraph_…` |
| `zz_vh1`: `h6_mixed_stop_runs` | `a_line_opening_with_a_bracket_pair_reads_its_stops_where_they_were_typed` |
| `zz_vh2t`: `h2_1_*`, `h2_6_*`, `h2_7c_*` | `a_styled_neutral_glued_to_a_word_under_spacing_reads_where_it_was_typed` |
| `zz_vh2t`: `h2_2_*`, `h2_7a_*`, `h2_7b_*` | `a_dirless_line_with_a_right_to_left_gloss` |
| `zz_vh2t`: `h2_3_*` | `a_word_of_an_overflow_script_an_inline_block_cuts_reads_as_a_latin_word_does` |
| `zz_vh2t`: `h2_4_*`, `h2_5_*`, `h2_7e_*` | `a_neutral_beside_a_number_or_a_styled_word_is_drawn_for_its_lines_reading` |
| `zz_vh2t`: `h2_7d_*` | `a_line_opening_with_a_bracket_pair_reads_its_stops_where_they_were_typed` |
