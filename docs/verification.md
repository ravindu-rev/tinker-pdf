# Verification

The posture in one sentence: **correctness claims are executable** —
never-panic is a fuzz target that never crashes, determinism is a committed
fingerprint, render quality is a perceptual budget, coverage of the real
world is a ratcheted corpus run, and a claim nothing executes is written
down as a claim.

`cargo test --workspace` runs every crate's unit tests, its integration tests
under `tests/` and its doc tests. The censuses that walk the fetched corpora
are `#[ignore]`d there and run in a local corpus run; a test that reads
anything else that cannot be committed prints a `RAN` or `SKIPPED` banner
(below).

## Where each check runs

**CI runs the unit and integration tests, on every pull request, and nothing
else** (`.github/workflows/ci.yml`): `cargo test --workspace`, the font crate
and the facade without their default features, and the facade with
`bundled-fonts`, on Linux and Windows. A merge to `main` runs the same tests
before it releases (below). Nothing is scheduled.

**Everything else is local proof**, run by whoever makes a change and
recorded in the roadmap row it closes:

| Check | How it runs |
| --- | --- |
| The gates — fmt, clippy, the tests, the determinism and shaping fingerprints, the examples, `cargo xtask check` | `python3 -I tools/gates.py` |
| Fuzzing, on Linux | `cargo fuzz build`, then `cargo fuzz run <target> fuzz/corpus/<target> -- -max_total_time=20 -timeout=20` a target |
| The fuzzing service's build, on Linux | `fuzz/service/Dockerfile`'s image built over `git archive HEAD`, its `compile` run, then `cargo xtask fuzz --service-out DIR` (`fuzz/README.md`) |
| The fingerprints on `wasm32-wasip1` | the roster's tests with `--target wasm32-wasip1`, wasmtime as the runner |
| The corpora | `cargo xtask corpus-fetch`, then `cargo xtask corpus-run --check` |
| The benches | `cargo bench -p tinker-pdf`, then `cargo xtask bench-check --machine NAME` |
| Instruction counts, on Linux | `cargo xtask bench-count --group NAME`, a group or a part of one at a time, with valgrind installed |
| Peak memory, on Linux | `cargo test --release -p tinker-pdf --test peak_memory -- --ignored`, in both builds |
| The bindings | each surface built, then `cargo xtask bindings-parity --require-all` |
| The wasm footprint | each profile built as `release.yml`'s npm job builds it, `RUSTFLAGS='-D warnings' wasm-pack build --release --target web --out-dir pkg bindings/js`, with `-- --features bundled-fonts` for that profile, then `cargo xtask footprint --profile NAME --gzipped BYTES --run REV@HOST --date DAY`, the bytes `gzip -9 -c bindings/js/pkg/tinker_pdf_js_bg.wasm \| wc -c` |
| Licences | `cargo deny check` |
| The oracle suites — the EPUB corpus, the VP8 vectors, the CommonMark spec, PngSuite | their fetch scripts, then their tests |

## Never panic, fuzz-enforced

Ruling 1 makes a fuzz crash a release blocker. cargo-fuzz targets in
`fuzz/fuzz_targets/` cover every input format, each landing in the same change
as its parser, and each has a seed corpus of its own name in `fuzz/corpus/`.

They run locally, on Linux. `cargo fuzz build` builds every target, and
`cargo fuzz run <target> fuzz/corpus/<target> -- -max_total_time=20
-timeout=20` replays a target's seeds and mutates from them for twenty
seconds; a longer session is the same command with a longer
`-max_total_time`. A change to a parser runs its target before it lands, and
an input a session finds is committed as a seed of that target.

The build a public continuous-fuzzing service runs is in the tree too,
`fuzz/service/build.sh` ([design/continuous-fuzzing.md](design/continuous-fuzzing.md)):
every target in the sessions' mode, its seeds packed from its corpus when the
build runs and its options the sessions' `-timeout=20`, so a crash or a
timeout the service reports is one a session here reproduces. It is built
locally the way the service builds it — `fuzz/service/Dockerfile`'s image
over the commit, and the image's `compile` — and `cargo xtask fuzz
--service-out DIR` reads what it wrote, naming every target whose binary is
missing or not an ELF file, whose seed archive does not read back through
`tinker_pdf_zip` to exactly the files of its corpus, byte for byte, or whose
options file does not set the sessions' timeout. The service's own check of
a build runs the targets and decides nothing here (ruling 13).

`cff_subset` is the one target that fuzzes a **writer**. Its assertion is
not "it did not panic": whatever the subsetter emits must parse with this
repository's own `Cff::parse`, every glyph asked for must still be at the
id it was asked for, and subsetting twice must produce the same bytes. A
subsetter that renumbered subroutine calls wrongly around the 107 / 1131 /
32768 bias thresholds produces a *valid* font that draws the wrong glyphs,
and no other check in the pipeline would see it — the embed path takes the
bytes and writes them into a `/FontFile3`.

### The roster, and whether the seeds reach the code

`cargo xtask fuzz` prints the roster — each target, its seed count, its
control prefix, its memory limits and stack, and its signature check —
derived from `fuzz/fuzz_targets/`, `fuzz/corpus/`, `fuzz/Cargo.toml` and
`fuzz/limits.tsv`, and fails if they disagree or a target has no seeds. The
same audit runs inside `cargo xtask check`, so the four lists cannot drift
apart without a gate going red.

`fuzz/limits.tsv` gives every target the `-rss_limit_mb` and
`-malloc_limit_mb` a session passes it and the stack it runs on, one row a
target. The figures are derived, never written:
`crates/tinker-pdf/tests/bounds_ledger.rs` names the buffers each target's
path holds and the bound each one has — a ledger cap at its shipped value, a
cap the ledger has no row for, a ceiling the target passes, or the input — and
computes the limits from them as
[design/resource-exhaustion.md](design/resource-exhaustion.md) states.
`the_fuzz_limits_are_derived_from_this_ledger` fails when the table differs
from the derivation over the live bounds, which is what a cap that moves
without it, a hand edit and a target ceiling that left its source all look
like, and `cargo test -p tinker-pdf --test bounds_ledger write_the_fuzz_limits
-- --ignored` writes the table again. The audit fails a target with no row, a
row for a name that is no target, a second row and a figure that is not a
positive integer, since a zero turns libFuzzer's limit off.

**The control prefix is the number of bytes a target takes off the front
before decoding**, parsed out of its own `split_at(data.len().min(N))` rather
than declared anywhere. It is where a whole corpus can be silently worthless:
if the seeds are written as *raw files* while the target eats a prefix, every
seed has its first byte swallowed and its signature destroyed before the
decoder sees it. The corpus then exercises the refusal path of every parser
and nothing else — and `cargo fuzz` runs perfectly clean, because a refusal is
a fine outcome for a fuzz target.

So the check is mechanical for every corpus whose format has a signature to
look for. The signature check reads `n/m carried` where the seeds put their
magic where the target will still see it, and the audit fails a corpus whose
signature sits where the prefix eats it. A corpus it cannot check reads
`unsignatured` — a raw coded stream, a structured generator, or text, all of
which genuinely have no magic — and every one of those is listed by name in
`xtask/src/fuzzaudit.rs`'s `UNSIGNED` table with the reason. **A target in
neither table fails the audit**, so a new corpus cannot be silently unchecked.

`unsignatured` is not the same as unmeasured. Replay harnesses such as
`crates/tinker-pdf-filters/tests/fuzz_seeds.rs` and
`crates/tinker-pdf-font/tests/fuzz_seeds.rs` count how many of a corpus's
seeds reach the code under test and refuse a corpus that reaches none. They
assert that **at least one** seed reaches rather than every seed, because a
corpus needs its refusal cases: `type1/truncated-eexec.pfb` does not parse on
purpose, and `truetype/cmap12-glyph-id-overflow.ttf` is exactly what its name
says.

`truetype` is held to two floors more, because composite recursion is what
the target's own header calls "the interesting part": a floor on the seeds
carrying an outline table, and one on those holding a **composite** glyph.
`crates/tinker-pdf-font/src/glyf.rs` writes the seeds that hold them up —
`simple-contours.ttf` for the flag and delta encodings of a simple outline,
`composite-transforms.ttf` for all five component forms `composite` reads
(word and byte arguments, a scale, an x-and-y scale, a 2×2 and the
point-matching placement that is rarest in the wild) and `composite-nested.ttf`
for four levels of a composite drawing a composite — so deleting one fails the
test with the count rather than passing quietly. They are floors rather than
an equality so that a corpus can grow without editing a test: the failure they
guard against is a seed *leaving*, deleted or quietly stopped parsing.
Composite-ness is read off `loca` and `glyf` inside the test, because the
public API cannot answer it: `glyf::outline` returns an `Outline`, and by then
a composite and the simple glyph it assembles are the same value.

`css`, `svg` and `xml` cannot be audited mechanically. A rule requiring the
body to be valid UTF-8 would fire on legitimate seeds — `xml/utf16` is a
UTF-16 document, which XML permits, and `css/repeated-class` carries raw
`0xFF` bytes on purpose to drive the parser's recovery — and a check with an
exception list is a check that has been talked out of firing. `svg` has a
replay of its own in `crates/tinker-pdf-svg/src/tests.rs`.

`cff_charstrings`, `content_stream`, `cos_syntax`, `crypt`, `crypt_ciphers`,
`form_script`, `glyf_outlines`, `layout`, `render_page` and `signatures` are
structured generators: the body is carved into fields rather than parsed as a
format, so every seed reaches the code by construction and there is no
reachability question of this shape to ask. The seeds of `cff_charstrings`,
`content_stream`, `cos_syntax` and `glyf_outlines` are choice sequences their
generators' `write_the_fuzz_seeds` write, each named for the feature it
reaches ([design/structured-fuzzing.md](design/structured-fuzzing.md)). `cos_document`,
`cos_object`, `content_tokenizer`, `icc_profile` and `zip_archive` are covered
by the signature check above or have no gate to fail at.

Two seeds look wrong and are not: `shape/gdef-every-structure` and
`shape_text/bidi-brackets` carry no readable face, and both targets have a
second path — `Layout::from_tables` and the bidi walk — that runs without
one.

### A fuzz run that never happened means less still

`fuzz/` is its own workspace, deliberately — it pulls in `libfuzzer-sys` and
builds under a sanitizer, and neither belongs in the engine's dependency graph.
The price of that separation is that `cargo test --workspace`,
`cargo clippy --workspace` and every other command a contributor runs step
straight past it, so an exhaustive match in `fuzz/fuzz_targets/` can stop
building while every local gate stays green. Ruling 1 says a fuzz crash is a
release blocker, and **a fuzzer that does not build cannot crash**: the rule
would read as satisfied while being unenforceable.

So `cargo xtask fuzz` — and therefore `cargo xtask check` — runs `cargo check`
over `fuzz/`'s own manifest, every target, the crate's library and its unit
tests included, and fails with the compiler's own error. It is
`cargo check` rather than `cargo fuzz build` because the question is whether
the *types* still agree; it runs on stable and needs no sanitizer, and
`cargo fuzz build` builds them for real.

### What a green fuzz run does not mean

**A fuzz target does not check that a decode is right.** The targets divide
into two shapes, and every target's own header says which it is under a
`# What this target ...` heading — which `xtask fuzz` requires, so a new target
cannot land without one.

**Some assert nothing beyond "it did not panic, hang, or exhaust memory."**
They call the decoder and discard the result with `let _ =`. A green run over
`render_page` is the single easiest result in this repository to mistake for
more than it is.

**The rest assert only structural or self-consistent properties** — a ceiling
held, a geometry agrees with its own sample count, a name past a cap was
refused, parsing twice gave the same answer, a reordering was a permutation.
Every one of those is satisfied by a decoder that is *consistently wrong*.
Three are worth singling out:

- **`shape` and `shape_text`** lean on determinism, and a shaper that is
  consistently wrong is perfectly deterministic.
- **`crypt` and `crypt_ciphers`** lean on a round trip, which proves the
  cipher is an involution and not that it is RC4 or AES. A self-inverse but
  wrong key schedule passes while producing documents no other reader opens.
- **`pki_cms`** enforces X.690's clauses about encoding and nothing about what
  the encoding says — and for a signature reader, a wrong answer reported
  confidently is worse than a refusal.

**The structure-aware targets assert what their generator wrote.**
`cos_syntax` writes files from a model of 7.3's objects and 7.5's sections and
holds the reader to reading them back: every object as written, a file written
clean on `Trust` with no warning and no structure-tier defect, and a lying
`/Length` recovered wherever 7.3.8.2 leaves it recoverable. Its residual is
`cff_subset`'s: the writer and the reader are both this repository's, so a
clause both misread round-trips perfectly; the corpus ratchet is what holds the
reader to files other producers wrote. `content_stream` asserts a relation: a
generated page's display list replays to the bitmap `Page::render` draws, every
byte and every warning — and a defect the two share draws the same wrong pixels
twice, so the analytic tier and the goldens remain what say a page is right.
`cff_charstrings` holds a generated CFF to its own writing: every glyph's
advance is the width it was written with, and a subset draws every glyph it
keeps as the font did, which reaches the subsetter's renumbering across the
bias edges with programs the byte-mutating `cff_subset` seldom builds.
`glyf_outlines` does the same for `glyf`, and counts contours: every glyph
must draw one closed contour for each contour it and its components hold.

`cff_subset` is the strongest target and still not exempt: it fuzzes a
*writer*, and its round trip goes through **this repository's own** parser, so
a misreading of the CFF specification shared by writer and reader round-trips
perfectly and draws the wrong glyphs everywhere else.

`brotli` asserts only self-consistency, and it is the one case where the
missing check **cannot** be supplied from anywhere in the tree:
`CONTRIBUTING.md` rule 1 leaves no encoder to round-trip against and ruling 13
bars asking another decoder. Everywhere else the check exists and is somewhere
else, and each header names where.

**A green mutating session is not evidence that a defect is absent.**
A twenty-second session replays the target's corpus and then mutates from it
for the rest of its time, so an input it reports was generated by the session
rather than committed, and a session that ends clean is a record of what that
session happened to reach. Coverage is uneven by design, too: a hardened password hash
runs a few executions a second where an ASCII filter runs tens of thousands,
and a clean result at the first rate is not the same evidence as one at the
second. A committed seed corpus replayed on stable by an ordinary test is
weaker again, and never counts as a session.

A corpus written by an `#[ignore]`d test in the crate that owns the fixtures —
any `fuzz/corpus/<name>` a crate test calls `fs::write` on — cannot drift from
those fixtures. A corpus with no writer, such as `jbig2`'s, holds inputs a
fuzzer found or a person wrote, replayed by a test (`jbig2_seeds.rs`) and
never regenerated, which is a different guarantee. A seed a bound stops is
held there by the refusal's name and by the count the decode stopped at, never
by a clock: `pattern-dictionary-of-one-pixel-patterns` stops at its 122 881st
pattern and `text-regions-re-reading-a-large-dictionary` at its 130 624th
element, each under the fuzz target's 64 KiB.

`icc_profile`'s first gate is the `acsp` signature at byte 36, so random bytes
reach its parser essentially never. Its seeds include profiles this build
*refuses*, deliberately: `NeedsLut` and `MissingTags` are reached after the
header check, the tag count and the whole `132 + i * 12` table walk, which is
where that format's arithmetic lives. Each seed states which it is and the test
asserts it, so a seed that quietly stopped parsing cannot sit in the corpus
looking like coverage it does not give.

Fuzzers need nightly, so a **hostile-input sweep runs on stable with the
tests, on every pull request**: seeded xorshift mutation of the real fixtures,
driven as an ordinary test (`crates/tinker-pdf/tests/hostile_input.rs`).

## Determinism, fingerprinted

Ruling 4's contract — same bytes, same options, bit-identical output on
every target — is proven by committed render fingerprints and document
byte-hashes in `crates/tinker-pdf/tests/determinism.rs`. Each fixture carries
an ink floor and an `UnreadableFont` guard, so a fixture that draws nothing
fails rather than becoming a baseline. The analytic tier's own pages are among
them, hashed through the same builders `render_analytic.rs` evaluates them
with, so the page a formula adjudicates and the page a hash covers are one
document. Details, and which targets are measured versus claimed, are in
[features/determinism.md](features/determinism.md).

## Corpora: fetched, pinned, never committed

`corpus/corpora.lock` pins every corpus by sha256, and those fetched from a
forge by upstream commit as well; a published archive with no repository
behind it has its checksum as the whole pin, and the lockfile refuses a
`commit` on such an entry rather than letting one pin nothing.
`cargo xtask corpus-fetch` fetches them. Nothing from any corpus enters git —
redistribution rights are per-file murky in every real-world PDF collection,
and a pin plus checksum reproduces the set without this repository becoming a
distributor. Licences per corpus are generated by `cargo xtask corpus-licences`
into [corpus/README.md](../corpus/README.md).

`cargo xtask corpus-run` opens and renders every file of every corpus, one
child process a file, run locally over the corpora `corpus-fetch` wrote and
against three committed bars. The first,
`corpus/ratchet.json`, is at 72 dpi with no font provider. Each file gets 180
seconds before its child is killed, unless its corpus states a timeout of its
own in the lock: SafeDocs carries `timeout = 240`, because a timeout is a
property of the documents rather than of the run, and documents off the open
web are far larger than a test suite's fixtures. A timeout is a limit on how
slow the engine is allowed to be; it is not a fix for being slow.

The runner reports each file as passed, failed, crashed, timed out or stalled.
A child killed at the timeout is **stalled** if it had written nothing for half
its budget and merely **timed out** if it was still naming pages: Rust's stdout
is line-buffered, so the capture file's length is a progress signal the runner
gets without a pipe or a protocol, and the report names the phase the child
stopped in (`at`) and the last line it wrote (`last`): `at` and `last` both
`page 1/5` is a child stopped inside page one's render, `last` `time page 1 …`
one stopped after the render returned, and `join 2/5` one stopped in page
two's structure join. `tpdf probe --stages` takes a page's render apart into
announced, timed stages — the page lookup, the document's fonts, the page's
images, its display list's recording, the list's draw and the draw with its
annotations, the last two cancelled after `--stage-limit` seconds with the
counts they reached — so the stage a render that does not return stops in is
the last one announced. No bar runs it. The limit of the signal is worth stating — one unit of work longer
than the stall window is silent for the same reason a hang is, so what the
runner claims honestly is *made no observable progress for half its budget*.

A record that never reached `done` — a child that crashed, timed out or
stalled — keeps what the child wrote about the document before it drew
anything: its `/Producer`, the pages it claims, the capabilities its objects
need and its structure tree's counts, so the file still counts for what it
carries and its record says how many pages it claimed. Only whole lines
before `phase render` are read, since a kill can cut the last line in half.
Its outcome, its missing peak and every verdict, span and join count stay a
finished child's alone.

The censuses run with the fetched corpora named in `TINKER_CORPUS` and
`TINKER_CORPUS_REQUIRED` set, under which a census that finds no corpus fails
by name rather than skipping, and each prints its own banner —
`pdfa-ledger: RAN ...`, `structure-census: RAN ...` — which is what says it
read them. `streaming_open.rs`'s three corpus tests are not `#[ignore]`d and
run by name.

Three more readings are made when a row asks for one.

- **A trace**, of a file as `<corpus>/<path>` with the path as `report.json`
  names it. `cargo xtask corpus-run --trace` runs that file's child alone,
  after the fetch, under 900 seconds: pages one and two, every stage of a
  render announced and timed, the draw stopped at 300 seconds with the counts
  it reached and, where a stage decoded one stream more than once, a
  `decoded` line naming the stream it decoded most, how many times, and its
  dictionary. The runner prints every line the child wrote, then `trace:` with
  the outcome, the phase it stopped in, its last line and, on Linux, the CPU
  time it had used when it was killed (from `/proc/<pid>/stat`), which tells a
  loop from a wait. A path no locked corpus holds is refused by name.
- **Another revision's verdicts.** That revision's own runner and child,
  built in a worktree of their own — the head's runner refuses an older
  child's records by their version — run the reported-breakage corpus in the
  three settings over the same fetched corpora, into `corpus/against/`.
  `cargo xtask corpus-diff
  BEFORE AFTER` compares each report with the head's file by file: per
  relation, how many files went from held and how many came back, each file
  whose verdict differs with both details, and the files one report lacks,
  named and not compared. It refuses two reports of different settings. With
  `--held-to-broke dpi` it prints only a row per file that went from held to
  broke, its corpus, its path and the file on disk; each is then printed
  object by object, dictionaries without stream data, with its probe.
- **Another revision's peaks.** That revision's code and the head's each run
  the five corpora with no faces, one after the other, over one fetch on one
  machine, and `cargo xtask corpus-peaks REPORT...` prints each corpus's
  largest peak in both reports, with its file, the children reporting, the
  phase that reached it and its file-backed share, beside the committed band,
  with each revision's `tpdf` size beside them. It prints and decides
  nothing.

Encrypted fixtures are opened with the passwords their upstreams state. `tpdf`
tries the empty password and nothing else, which is right for a tool and wrong
for a census, so [corpus/passwords.tsv](../corpus/passwords.tsv) carries a row
a file, each quoted to the upstream line that states it, and the runner passes
each to the child. A row marked `derived` was opened by trying what its
siblings use, because no upstream file names it, and the word is there so
nobody reads a derivation as a quotation. One row names a gap rather than
closing it: `saslprep-r6.pdf` is pdf.js's test that a revision 6 password is
SASLprep-normalised before it is hashed (ISO 32000-2 7.6.4.3.3), and this
engine does not do that, so the sidecar carries the normalised form and says
why — the rest of the file is measured, and a missing feature does not look
like a present one. Reading an upstream test script for a password is an
*input*, which ruling 13 admits.

The first axis is the pass rate: files whose every page rendered. The second —
files rendering *with something reported* — is measured without font faces,
and two more bars say what that costs: `corpus/ratchet-fonts.json` is the same
files with a synthesised face supplied, and `corpus/ratchet-bundled.json` the
same files with the Liberation faces the `bundled-fonts` feature ships.

The synthetic face is one this repository writes for itself (`cargo xtask
synth-face`) — every glyph from 32 up a filled box — so it answers *was a face
available* and nothing more, needs no licence and no download, and is the same
bytes on every machine. Its bar flatters itself: a symbolic font that the
bundled set declines is answered by that one all-purpose face with squares
([features/fonts.md](features/fonts.md)).

The three bars live in three files and `corpus-run` refuses to compare any
against another — and refuses harder than that for the bundled one, because the
child's faces are a property of how `tpdf` was compiled rather than of the
command line. A `tpdf` built with the feature measures every file with its
faces whether or not anybody asked, so it reports its own `cfg` in its record
and the runner fails the run when that and the `--fonts` setting disagree, in
either direction.

The third axis is ruling 13's, and it is about the **writer** rather than the
reader: every file this engine read cleanly is rewritten in memory and the
rewrite is handed to the strict validator. Only its structural tier counts —
the header, the cross-reference sections, the offsets, the stream extents, the
trailer — because a rewrite copies the page tree, the annotations and the
resource dictionaries from its source, and a backwards `/Rect` in somebody's
invoice belongs to the invoice. A file this engine could not read cleanly
is not eligible and is counted as neither, and its record names the repairs
that made it so, each kind once with the object it was first raised on: a
leniency a page's drawing needed counts as surely as one the open needed,
since objects load lazily. What the semantic tier finds in the
same rewrites — outline counts that disagree with their own trees, backwards
annotation rectangles, malformed `/W` arrays — is listed per file and per rule
in the run's `corpus/report.json`, and belongs to the documents. The pass sees
writer defects nothing else here does, because this engine's own reader
repairs them in silence.

The ratchet: recorded rates may not decrease; improving one updates the
recorded floor in the same change. A run with more files stalled, or more
crashed, than its bar's `outcomes` counts fails by name, each stall at the
phase it stopped in and each crash with what its child last said, whatever
the pass rate does; a timeout stays a note beside the pass rate, since a file
near its limit flips with the machine's load. The floor and the budgets live in-repo,
diffable, so lowering one is the owner's decision (H-78) rather than a drift.

**Time is held to a band rather than a floor.** Each child times its open and
count and each page's render (`tpdf probe`'s `time` lines), and per corpus the
run reports pages a second at its dpi, the mean open-and-count latency, and the
50th, 95th and 99th percentile page render over the corpus's pages, with its
ten slowest files under their `/Producer`. A bar's time figures come from five
runs at one revision and never from one run, so `--record` carries them
over unchanged; `--check` holds each figure to the band those five set, refuses
a run at another dpi or job count, and reads a bar without them as new.
[design/performance.md](design/performance.md) has the band rule.

**A peak band is three runs'.** `cargo xtask corpus-peaks --record BAR
--revision REV RUN=REPORT...` writes each corpus's `peak` in a bar from the
reports of three corpus runs at one revision: the
largest of their maxima, over the children they agree reported one, with the
runs, the hosts they landed on and their revision in the bar's `peak_runs`.
It refuses, each by name, runs under other settings than the bar's, runs that
disagree on a corpus's children or report fewer than its band does, and maxima
that spread by `PEAK_TOLERANCE_PERCENT` or more; such a spread or denominator
is the owner's to judge (H-78), never banded wider. A corpus the owner holds
to no band is written with the reason in its place,
`--unheld CORPUS=REASON` giving it, and `--check` prints that corpus's
largest peak beside the reason and compares it with nothing, as
`peak_memory.tsv` holds a case it names `unheld`. Every band in the three
bars, and every time figure, is the hosted runner's, read from the runs each
bar names; held against another machine, `--check` measures that machine as
much as the code, so a time or peak line of a local run is read beside the
machine before it is read as the engine. `corpus-run --record` writes no peak
from its own run and carries each committed band, each unheld reason and
`peak_runs` over unchanged, as it does the time figures.

**Where a peak is reached.** On Linux each child writes the kernel's
high-water mark as each phase of its work ends — `open`, `scan`, `render`,
`strict`, `metamorphic`, and inside the last the render the relations compare
with (`meta-base`), each relation's rewrite, reopen and render
(`meta-rotate-rewrite` to `meta-crop-render`) and the doubled render
(`meta-dpi`) — and, before its `peak`, the file-backed part of its
resident set, which is the binary's own pages rather than what the document
cost. The report keeps both per file, and each corpus's `peak` line names the
file that reached the largest peak, the first phase whose mark reads it and
its file-backed share. A bar holds the peak alone.

**Each part's peak is its own memory.** The platform allocator keeps what a
part of the work frees, resident, and what it keeps turns on where each
allocation landed, which moves from run to run with the hash seeds the
engine's maps draw and with the address space's layout. Left there, it
decides a later part's peak in that part's place — the part reuses it or
adds its own memory beside it, and the doubled render of one document adds
its canvas on one run and nothing on the next. On Linux with glibc a child
therefore hands what the allocator holds free back to the kernel
(`malloc_trim`) as it reads each mark, and again before the crop's rewrite
and the doubled render, which start after the part before them has dropped
its documents, so each part starts from live memory alone; the doubled
render is marked while it is held. The kernel keeps its high-water
mark from counters it folds in batches, and it can read under a mark the
child already took, so `peak` is never under one of the child's own marks.
And `tpdf`'s loadable segments ask for 64 KiB on Linux
(`tools/tpdf/build.rs`), the window a page fault maps around itself, so the
kernel loads the binary at a boundary of that window and each run maps the
same pages of its code; the shared libraries keep the placement their own
builds ask for. `tools/tpdf/src/doubled_peak.rs` holds the doubled render to
its own memory, a process for each of a generated one-page document and a
ten-thousand-page one: after nothing, after the relations that rewrite, and
after small blocks freed in the order they were made and in reverse, the
resident set with the doubled render held reads the same, within a
mebibyte, under the main arena alone as the probe's one thread allocates.
`tools/tpdf/tests/load_alignment.rs` reads the segments' alignment from the
built binary.

### The fourth axis: relations between two renders

The corpus has no ground truth — nobody here knows what its pages should look
like — but a *relation between two renders of one file* needs none, and it can
be asked of every file at once. `tpdf probe` checks three in-process, on the
first page, and reports **four** judgements — `rotate` is judged twice, against
the budget that decides pass or fail and again against a tighter line that
decides nothing — and `corpus/ratchet.json` records how many files each was
asked of beside how many it held on, and which files it broke on by path, so
a check whose relation falls names each file it broke on that the bar does
not list, with what its child measured, and a rise names each listed file
that holds now. A bar recorded before the lists compares by count and says
it names no file; `cargo xtask corpus-names REPORT...` gives one its lists
from a report of its own revision, refusing a report whose tallies are not
the bar's, so no count moves. What declines a relation for a file is a
property of the document, so how many files it was asked of is a function of
the corpus rather than of the machine; the per-file report carries each
document's `cost` — bytes, objects and first-page pixels.

Every hold carries its own measurement, `held 12 of 501832`, rather than only
`held`: the budgets are sited from a population, and a record that keeps only
the failures throws away what a budget would have to be re-sited against.

- **`rotate`** turns the page a quarter and requires the transposition, within
  a budget of **10 %** of pixels. It is not exact: turning the page puts every
  glyph and every anti-aliased edge on a different sampling grid.
  `crates/tinker-pdf/tests/metamorphic_classes.rs` puts constructs through this
  relation one class at a time, and two classes move anything: a tiling
  pattern, whose lattice rounded wrongly is a defect this relation must go on
  catching, and anti-aliased edges that are not axis-aligned, which is
  arithmetic. A budget is a share of a page, so each construct is measured
  covering one, and ten per cent sits above the widest legitimate construct —
  a page of text — and below the defects the relation is for.

  A budget wide enough to admit a page of text sees less, so the same `moved
  of total` is judged a second time at 3 % and recorded as its own relation,
  **`rotate-tight`**, which the ratchet holds like any other and which no run
  fails on. A change that makes glyph edges noisier moves it long before it
  moves `rotate`; a rotation applied to the geometry and not the clip moves
  both.

  One limitation no budget removes, asserted in that file rather than left to
  be rediscovered: a page saturated with diagonal edges costs as much as the
  tiling class does, so at no budget does this relation separate a saturated
  vector page from a lattice rounded wrongly.
- **`crop`** moves the page box and requires the sub-rectangle of the full
  render, **exactly**. It needs no budget because it changes no sampling grid.
- **`dpi`** renders at twice the scale, box-filters down and requires
  agreement within 2 %.

Both counts are recorded and both are ratcheted, for the strict pass's reason: a
relation that quietly stopped being asked would otherwise raise its own hold
rate by shrinking what it is a rate of. `rotate` and `crop` are asked only of
files this engine read cleanly, because a rewrite of a document the reader had
to repair compares two repairs rather than two renders.

**What a placeholder does to them, because it looks like a pass.** A capability
the engine declines draws a neutral rectangle, and a neutral rectangle
transposes exactly, crops exactly and resamples exactly — it holds every
relation on this page perfectly. So building a missing capability can cost a
file a relation, and that is content appearing rather than a defect arriving:
it shows as a relation lost in the same run as an `UnsupportedImage` or
`UnreadableFont` warning. The relations are a check on the engine's
arithmetic, not on how much of a document it can draw, and the degraded count
above is the measure that moves the right way.

**What they cannot catch, stated because it is why they are an axis and not a
verdict:** any defect that commutes with the transformation. A colour converted
wrongly is converted equally wrongly at both resolutions and at both rotations,
and every row above stays green.

**The honest limit of the corpus run**: it measures whether a bitmap came
back, not whether it is the right bitmap. Nothing here compares a page this
engine drew against a page anything else drew, and under ruling 13 nothing
ever will. [design/render-verification.md](design/render-verification.md)
says what stands in its place and, precisely, what that cannot reach.

## Verification is first-party

Ruling 13. Nothing outside this repository renders, parses, validates or
measures a document as evidence. A third-party program may host this code,
execute it, fetch bytes for it or generate inputs for it; it may never be
the thing that says whether the output is right. Third-party **bytes** stay
admissible with provenance recorded — the fetched corpora, fixtures real
producers emitted, published normative data (Adobe's CMap resources, the
Unicode character database), and the committed output of a tool that was run
once, which is a dated measurement rather than a check.

No test in this repository invokes an outside program to
adjudicate a document, and `cargo xtask oracles` keeps that from eroding:
every file that spawns a program has a row in its `SPAWNERS` table saying why,
a file that spawns without one fails the build, and so does a row whose file
spawns nothing. What stands where an outside program would:

| What is checked | How |
| --- | --- |
| Written output is a well-formed PDF | The strict validator (below), plus `validated_output.rs`, and `cbz_validated.rs`, `xps_validated.rs` and `epub_validated.rs` reading every value back out of the dictionaries |
| An XPS package's markup reaches the document | `xps_conservation.rs`: a census of the markup by an independent in-test walk, against a census of the document synthesised from it |
| CSS layout | `epub_analytic.rs`, whose every expected number is computed in the test, and `epub_reftest.rs`, which lays out pairs the specification says are one document |
| Whose fault an engine-versus-book EPUB disagreement is | Nothing. `tests/epub/EPUBCHECK.tsv`'s verdicts stand as a dated record, never re-run |
| JPEG 2000 decode | Committed reference decodes, and the standard's own published numbers |

### PngSuite, an input that adjudicates both directions

Ruling 13 removes *programs* from the verification surface and keeps *inputs*,
and PngSuite is squarely the second: Willem van Schaik's 176 files say what a
PNG implementation should do by being deliberately correct in 162 documented
ways and deliberately broken in fourteen. Nothing of it is committed, and CI,
which fetches nothing, skips it. It runs locally, fetched once into a
directory of its own, with `TINKER_PNGSUITE` pointing at what it extracts to:

```sh
curl -LO http://www.schaik.com/pngsuite/PngSuite-2017jul19.zip
unzip -d pngsuite PngSuite-2017jul19.zip
TINKER_PNGSUITE=$PWD/pngsuite \
  cargo test -p tinker-pdf-filters --test png_suite -- --nocapture
```

`crates/tinker-pdf-filters/tests/png_suite.rs` prints `pngsuite-oracle: RAN` or
`pngsuite-oracle: SKIPPED` per test, because a skipped check exits 0 and reads
exactly like a pass, so a run is proof only where every one of its tests
prints `RAN` and none prints `SKIPPED`.

**It adjudicates the decoder**: the filename convention is an independent
statement of every IHDR, the fifteen `bas*` pairs are Table 11.1's fifteen
legal pairings, `basn`/`basi` twins are fifteen independent confirmations of
Adam7, the `oi*` group is one zlib stream cut into 1, 2, 4 and n IDAT chunks,
and each of the fourteen broken files must be refused *by a name matching the
published reason*.

**It adjudicates the encoder too**, and that takes two legs rather than one.
Leg one decodes each of the 162 readable files, encodes the raster, decodes
that and requires the same pixels — so the raster the encoder is handed came
from an encoder nobody here wrote, at every legal colour-type/depth pairing,
interlaced and not. Leg one alone would not be worth much: `png/encode.rs`
filters with the same `predictors.rs` Paeth predictor `png.rs` unfilters with,
so a defect in the shared predictor moves both directions together and the
comparison stays green. **Leg two transcribes ISO/IEC 15948 9.2's five
reconstruction formulas into the test** and rebuilds the raster from the
encoder's own IDAT without calling `png_decode` at all — first-party, which
ruling 13 requires, and a second *implementation* rather than a second
*program*, which is the line that ruling draws.

A defect the two directions share, such as a reversed Paeth tie-break, passes
every round trip of leg one. Leg two catches it, and so do the decoder tests
that compare two *third-party* files against each other — the Adam7 twins and
the published equivalence classes — because a shared defect has nothing there
to cancel against. The container is held to clause 5 on the way past —
signature, IHDR first, IEND last and empty, every CRC recomputed over its
chunk's type and data, and the IDAT payload equal to the zlib stream the
writer's fast level makes of the filtered stream
(`zlib_compress_at(stream, DeflateLevel::Fast)`).

What PngSuite cannot see is the `Bitmap` side of it — it has no `PixelFormat`
and no notion of ink or `L*a*b*` — so the format mapping is
`crates/tinker-pdf/tests/png_output.rs`'s, and the two converted formats are
asserted there by colour rather than by comparison.

### The strict validator

`tinker_pdf_cos::validate` reads a file again with the leniency ladder **off**
and holds it to ISO 32000. The design decision that makes it worth anything is
what it is *not* allowed to read:

- not the typed readers. `read_shading` defaults a missing `/Extend`,
  `parse_function` defaults a missing `/Domain`, the page walker invents US
  Letter for a missing `/MediaBox` — a document built out of those omissions
  round-trips through this crate perfectly and is not a PDF anybody else can
  read. A unit test reads the module's own source and fails the build if it
  ever imports one of them.
- not the merged cross-reference table, which is what the ladder decided
  rather than what the file says: an entry pointing a few bytes before its
  object, or a generation the table invented, opens at `Trust` with no warning
  at all. So the validator walks the cross-reference **sections** out of the
  bytes, one `/Prev` link at a time, in both spellings, and holds every entry
  to the byte it names.

Rules are split into two tiers, and the split is a claim about *whose* defect
it is. **Structure** is what a writer owns whatever document it was handed;
**semantics** is what the document says, which a rewrite inherits. Only the
first is ratcheted over the corpus. Annex F's hint tables have their own
decoder, written from the tables rather than from `linearize.rs`'s plan, and
every number it reads back is compared against the file's own object extents.

`crates/tinker-pdf-cos/tests/strict_validator.rs` holds each rule to a defect
put into a document this engine wrote, which must be refused by the rule's
name, and asserts the undamaged twin clean in the same file.
`tpdf check --strict` exits by the verdict.

### XPS conservation

`xps_conservation.rs` asks one question of every committed package: **does the
synthesised document carry what the markup states, in order, at the place
ECMA-388 18.1 puts it?** Both sides state the same census — page sizes, painted
elements with their colours, gradient geometry and stops, image pixel counts and
rectangles, tiling copies, glyph runs with their origins, sizes, text and the
advances `Indices` overrides — and the comparator reports a typed divergence for
each thing that differs. Every committed package conserves every fact, and the
counts are recorded in `tests/xps/CONSERVATION.tsv`, which a change that moves
them has to re-measure.

What makes it worth anything is the same thing the strict validator's value
rests on: **what the markup side is not allowed to use.** It reaches for
`tinker-pdf-zip` to get bytes out of the package and nothing above that — not
the XML parser the reader parses with, not the PNG and JPEG decoders that give a
picture its pixel count, not `geometry`'s reader of 11.2.3's abbreviated
grammar, and not 18.1's scale and flip, which are written out from the clause so
that a defect in the reader's arithmetic cannot cancel against a harness sharing
it. A harness that asked the reader what the reader decided would have checked
nothing.

The order the painter composes its open scopes in has a fixture of its own,
`a_canvas_scale_over_a_child_translation_composes_the_child_first`: two nested
canvases that do not commute, with an image brush beneath them. Translations
commute, and the outermost open scope is the page, whose transform is the
identity, so neither a package of translations nor a single canvas can part
the two orders.

### EPUB analytic layout, and reftest pairs

Two files hold EPUB layout, and they are two different kinds of evidence.

**`epub_analytic.rs` computes the answer.** `BookMetrics::STANDARD` measures
with the standard 14, and Courier's advance is exactly 600/1000 of the em for
every character — a number in the specification's own Appendix D rather than
one this repository chose. So a document set in `monospace` has a line breaker
with a closed form, and every expected value in the file is arithmetic the test
does: `floor(measure / 0.6 × size)` characters to a line, baselines
`line-height` apart, adjoining margins collapsed to the larger, padding and
border adding on the content edge, `text-indent` on the first line and no
other, a float shortening the lines beside it and none below, and CSS 2.2
§13.3.2's `orphans` and `widows` deciding where a column breaks.

**`epub_reftest.rs` needs no answer at all.** It lays out pairs of documents the
specification says are one document — a `margin` shorthand against its four
longhands, `1.5em` against `24px`, `50%` against `120px`, `<b>` against a bold
`<span>`, an implied `<tbody>` against an explicit one, two collapsed margins
against the single larger one — and requires that this engine agree. Nobody has
to know where the boxes go. Every pair carries a **mismatch reference**: the
same side with one declaration changed, which must *fail*, because a pair that
cannot disagree proves nothing and two empty documents agree perfectly.

### Rasters that answer to arithmetic

The tier [design/render-verification.md](design/render-verification.md) calls
*"the only tier that answers to mathematics"*, and where it applies it is worth
more than a second renderer: an expected value computed from the geometry has no
opinion to be wrong about.

`tinker-pdf-raster/tests/analytic_coverage.rs` states the sampling grid —
**sixteen sub-scanlines a row, exact horizontal spans in 1/256 of a pixel** —
and computes every expectation from it. A rectangle on integers is exactly
covered; half a pixel is 128 whichever axis the fraction is on; a corner cut on
both is their product; nonzero minus even-odd is exactly the intersection; a
diagonal edge is sixteen spans summed from the line's own equation; a stroked
segment is length times width plus a cap term that is zero, a square, or bounded
between the disc and the square inscribed in it, because a round cap is
flattened to chords before it is filled and so is strictly smaller than `πr²`.

`crates/tinker-pdf/tests/render_analytic.rs` does the same one level up, where a
fixture needs a content stream: both shading types compared **per pixel across
the whole page** against 8.7.4.5's parametric equations, the twelve separable
blend modes of 11.3.5 over nine backdrop-and-source pairs, and an image at an
integer scale as blocks of its samples.

Three conventions are written down where the fixtures use them: a component
reaches the page as `round(v × 255)`; a shading is sampled at the pixel's
**centre**; and blending happens in **eight-bit fixed point** on both operands
— `blend.rs` has no floating point in it at all, so ruling 4's bit-identical
contract does not rest on anybody's `sqrt`. Eight of the twelve modes
reproduce the clause exactly and the four that divide or take a root are
within one level, and the fixture says which four, so a mode leaving the exact
list is a visible change rather than a tolerance quietly absorbing it.

`coverage_follows_the_grid_between_its_own_gradations` holds the grid between
its gradations as well as on them, because an accumulator that rounds instead
of truncating, or a sub-scanline taken at its floor rather than its ceiling, is
invisible at every whole sixteenth of a pixel.

`tests/epub/EPUBCHECK.tsv` holds what epubcheck 5.3.0 said about the committed
books when it was run: a dated measurement that nothing re-runs, so when this
engine and a book disagree there is no arbiter to say whose fault it is. What
the record does is hold the corpus to having a verdict per book and to the set
of books the tool was unhappy with, so a book added without one is a visible
gap.

`jpx_reference.rs` invokes nothing either: it compares against `.opj.pgm` files
committed once, with the commands, the tool version and the date in its
header.

**Beside it is first-party evidence in the strongest sense available: the
standard's own published numbers.** T.800 Annex J.10 states a
100-byte codestream, its intermediate coefficients (J.10.4) and its nine
decoded samples (J.10.5), and `jpx_annex_j.rs` asserts them.
`jpx_annex_h.rs` extends the same numbers to Annex H's region of interest,
which T.800 publishes no test data for at all: H.1 rewrites exactly the
coefficients J.10.4 prints, so the clause can be run over the standard's own
values and held to the standard's own samples.

`jpx_poc.rs` extends them a second way, and it is a different lever worth
naming. A.6.6's progression order change acts on packet *order* rather than
on coefficients, and T.800 publishes no codestream carrying one — `0xFF5F`
occurs exactly twice in the 231 pages, in Table A.2 and Table A.32. What
J.10 publishes instead is **where each of its two packets begins and ends**
(J.10.3's Table J.20 and J.10.4's octal 0125; Table J.21 and octal 0137), and
a published packet boundary is what a reordering needs: the file swaps
J.10's own nine and seven bytes, describes the swap in a two-volume POC, and
demands J.10.5's nine samples back. It also shows what ignoring the POC would
cost: the identical swapped bytes with the POC removed decode **cleanly and
silently** to nine different samples.

`code_block_styles.rs` extends them a third way, and this lever is Annex B's
rather than Annex J's. Table A.19's last two code-block styles — D.6's
selective arithmetic coding bypass and D.4's termination on each coding pass —
act *inside* a packet, which no rearrangement of J.10 can reach. But
**B.10.7.2's own NOTE publishes a worked example**: a code-block's five
included passes, the set `T` of terminated ones D.6 produces, the four lengths
that are then signalled, their pass counts, and a valid 39-bit sequence coding
all of it. That is the standard's bits in and the standard's numbers out, fed
to the shipped packet-header reader, with `T` derived from this build's Table
D.9 transcription rather than written into the test — so the four lengths only
come back in order if the transcription puts terminations where Table D.9
does. B.10.7.1's NOTE 1 does the same for the single-segment case, four layers
deep, including two `Lblock` increments.

J.10 then brackets D.6's boundary without moving a byte, because `K` — how
many lengths a packet header signals — is a function of the pass count. J.10's
second code-block has seven coding passes, all before the boundary, so its
published header must read identically with the style bit set and its three
published bytes must still give J.10.4's "1, 5, 1, 0"; its first has sixteen,
which straddle it, so B.10.7.2 wants five lengths where Table J.20 prints one
and the published header stops parsing. Together those put the boundary in
7..=15, and ten is transcribed from D.6 and Table D.9.

**What this does not cover is `TERMALL`, and the file says so in its header.**
T.800 publishes no codestream with a raw or per-pass-terminated coding pass in
it, and J.10's two code-blocks carry sixteen and seven passes against headers
printing one length each, so `TERMALL` cannot be flipped onto them: sixteen
and seven lengths would have to be re-encoded, and an encoder this repository
wrote agreeing with a decoder this repository wrote is a statement about this
repository. `TERMALL` is held up by Table D.8 transcribed and asserted as data,
by B.10.7.2's rule turning that table into `K`, and by sharing that one
function with the half B.10.7.2's NOTE does adjudicate. That is weaker, and
naming the weaker half is the point of this section.

None of the four files invokes anything and none is a round trip through an
encoder written here. One test in the fourth is nonetheless a comparison of
two inputs this repository authored — `a_contribution_split_across_two_layers`
builds the same twelve code-block bytes as a one-layer and a two-layer
codestream and requires the same picture — and it says so in its own doc
comment. It is there because B.10.7.2's multi-layer merge is stated in prose
with no bytes published for it. A statement about bookkeeping, checked against
itself, is worth having and is not worth calling evidence.

**A picture compared against an outside viewer is admissible as an input and
never as a check**, as `jpx_reference.rs`'s committed decodes and
`tests/epub/EPUBCHECK.tsv`'s verdicts are. It may be recorded with the tool,
its version, the command and the date; it may be read by a person and cited in
a document; it may not be re-run, may not gate a build, and may not be called
a test. A dated measurement is evidence about one day, it decays visibly, and
nobody has to trust it tomorrow; it establishes none of the properties below.
Tier 1's reviewed goldens remain the only first-party answer to "is this the
picture the clause describes", and they are worth exactly what the review that
produced them was worth.

**What first-party verification cannot prove.** Four properties need a program
nobody here wrote, and no first-party mechanism establishes them:

1. A reader nobody here wrote accepts a PDF this engine wrote.
2. An independent program's reading of an XPS package matches this engine's.
3. This engine's CSS layout matches the reference implementation of CSS —
   both sides of a reftest share every misreading its author has.
4. When this engine and a book disagree, there is an arbiter.

The risk under all four is one risk: reader, writer, validator, fixtures and
reviewer share one author's reading of the specification, everything agrees,
and everything is wrong — deterministically, on every target, forever. It is
not closed, and nothing here closes it. What narrows it is the part of the
suite that answers to something other than this engine's own opinion:
fixtures whose correct output is computable in closed form, bitstreams
transcribed from the standards' own annexes, published conformance data, and
thousands of documents nobody here authored.

**A check that can be absent is red, not green.** A missing binary, an
unfetched corpus or a skipped test exits 0 and reads exactly like a pass, so
every check that depends on something being present prints a `RAN` /
`SKIPPED` line, or a count of what it measured, and a local run of it reads
that line before it counts as proof. The checks that fetch what cannot be
committed need it most: what they read can fail to arrive as well as fail to
read, and both look like a green tick. In a corpus run the strict pass's line
proves it ran, because a child that skipped it would leave every file "not
eligible", which reads as a run that measured nothing rather than as a
regression; the gates' `determinism` and `shaping` gates require the lines
naming each fingerprint test, because a suite that ran says nothing about
whether a given test was in it, and `cargo xtask determinism` holds every
roster test to being a `#[test]` with no `#[ignore]`.

## The release

A merge to `main` releases. `release.yml` runs on every push to `main`: CI's
tests first, then `cargo xtask check` and one question, whether the workspace
version has a `v<version>` tag yet. If it has, the run stops: that version is
released, and a merge that does not change the version releases nothing. If
it has not, every artefact is built and checked — the crates packaged and
verified against each other, an abi3 wheel for Linux, Windows and macOS, the
npm package under its size gate, the NuGet package with three native
libraries, the browser demo — and counted, and then published: the crates to
crates.io in dependency order (`cargo xtask release --execute --only
crates`), the wheels to PyPI, the package to npm and to NuGet, each skipping
what the registry already has, and last the tag and a GitHub release, so a
version is tagged only once every registry holds it and a run that fails part
way is finished by the next.
`cargo run -p xtask -- release --local-registry` makes the crates stage
provable: `cargo publish --dry-run` resolves a crate's workspace dependencies
against the live index, so each crate is verified instead against the unpacked
`.crate` archives of the crates before it, which are the same bytes its
dependents would download.

## Bounds are measured against real inputs

Every hardening cap is a row in `crates/tinker-pdf/tests/bounds_ledger.rs`,
each carrying a figure for a real book, comic and document rather than a
guess. A figure that is arithmetic about a plausible file rather than a
measurement of one publishes the word **estimate** in the number itself,
because a row that reads like a measurement when it is not is the failure this
file exists to prevent. The ledger also asserts that no bound is proved by a
clock — `Instant::now` is banned from every source a row names as firing it —
so the caps are properties of inputs, not of machines.

## Tools

- **`tpdf`** (`tools/tpdf`): the CLI over the facade. Its read commands are
  `info`, `text`, `render`, `fields`, `fonts`, `images`, `outline`,
  `objects`, `check` and `probe`, and its write commands `merge`, `split`,
  `rotate`, `encrypt`, `decrypt`, `attach`, `stamp` and `sanitise`.
  `check --strict` runs the validator and exits by its verdict; `probe` is
  what the corpus runner spawns, and its record carries the strict pass.
  `render` writes `<stem>-NNNN.png` through `Bitmap::to_png` — PNG always,
  with no flag for anything else, because a debug tool with two output paths
  has one that is rarely taken and eventually wrong. The writers are wrappers
  over editor calls that save through `write::save`, so each takes
  `--font-policy` and subsets by default (`tools/tpdf/src/writing.rs`). Their
  tests call each command's function rather than spawning the binary, reopen
  what it wrote through the facade and hold it to the strict validator: pages
  and text in order after a merge, a resource three pages share written once,
  a split piece carrying none of the pages it left out, rotations on top of
  the existing turn, a whole face cut by default and kept byte for byte under
  `keep`, each password opening an encrypted file with its own authority and
  the same `--entropy` giving the same bytes, an owner's restriction refusing
  `decrypt` from the user, an attachment's bytes and `/Subtype` back as given,
  a stamp's text over or under the page's, and each thing `sanitise` takes out
  gone from every stream — each of its flags alone taking out only what it
  names, and no flag at all refused. The image flags are held the same way: no
  flag leaves every stream as stored,
  `--images flate --bilevel g4` gives back exactly the samples written,
  `--max-ppi 36` halves a 72 ppi image and names the one-bit image it would
  not box-filter, and `--images jpeg` writes `DCTDecode` with the tables
  named. What a writer would ignore is refused with nothing written — a flag
  it does not take, `--password` on an input that is not encrypted, and a page
  or a split piece the list names twice.
- **`pdfcmp`** (`tools/pdfcmp`): the canonical perceptual comparator. Gates
  on the fraction of pixels where any channel moves more than a threshold —
  a glyph moving one pixel barely moves a mean, so the metric is changed
  pixels, not mean difference. `--diff` writes a per-pixel heat map beside
  the verdict, because a number that fails without a picture wastes a
  human's morning. It reads a `.png`, a `.pnm` or a `.pdf`, recognising each
  by its signature as well as its extension, so `tpdf render`'s output feeds it
  directly. The PNG goes through `Bitmap::from_png` on the facade rather than
  through `tinker-pdf-filters`' decoder, which `xtask`'s `TOOLS` table keeps
  out of a tool's reach; a PNG carrying alpha is compared as it looks over
  white, and one whose raster stops short is refused rather than scored.
- **`cargo xtask`**: `dag` (crate-graph enforcement), `libm`
  (transcendental ban on pixel paths), `oracles` (ruling 13's boundary: every
  program this repository spawns is written down with a reason), `vendor`
  (data-file provenance gate), `versions`, `names` (the roadmap names no
  engine, viewer, validator or document tool), `check`, `corpus-fetch` /
  `corpus-run` / `corpus-licences`, `release`, `nuget-stage`.

`cargo xtask names` holds the roadmap to the owner's answer on how far the
never-name rule reaches (ruling 23, H-07): `docs/ROADMAP.md` and every file
under `docs/roadmap/` name no engine, viewer, validator or document tool,
and each corpus by its role, while the provenance files keep the names. The
names themselves are not written in the tree. `xtask/never-named.sha256`
holds the SHA-256 of each — engines, viewers, validators, document tools,
browsers and OCR engines, and the names of the fetched corpora and of the
suites they hold — one a line and sorted, each lowercased and a two-word
name as its two words joined by one space, written from a candidate list
that is never committed; a name that is also a word the roadmap uses enters
only in its two-word form. The scan splits each file on every character
that is not a letter or a digit, line breaks included, hashes every word
and every pair of adjacent words with `tinker-pdf-crypto`'s SHA-256, and
fails on a listed hash with the file, the line and the hash, never the
word; nothing exempts a line. A hash of a short public name is reversed by
hashing candidate names, so the list keeps the names out of the roadmap
and claims nothing about keeping itself secret, and a name it lacks is one
more line.

## Clocks: outside the suite, and not nowhere

Clocks are banned from **tests**. `bounds_ledger.rs` bans `Instant::now` from
every source its rows name, so that every hardening bound is a property of an
input rather than of a machine, and the guard suite bans `Instant::now` and
`SystemTime::now` from every source it is built from or names.

They live in `crates/tinker-pdf/benches/` instead — one file a group over a
shared `support/` module, under criterion, which is exempt tooling by name and
whose plotting stack is switched off. `cargo bench` runs locally, one group at
a time with `--bench <group>`, and never on a pull request: a machine swings
between two runs of identical code, and a benchmark that fails a pull request
on that teaches people to ignore it. The useful mode is criterion's own
comparison, which needs two revisions on one machine:

```sh
cargo bench -p tinker-pdf -- --save-baseline before
cargo bench -p tinker-pdf -- --baseline before
```

and `--bench <group>` narrows either to one group's file.

Twenty-seven operations time `tpdf` itself: those tier P marks CLI, and the
`threads` group's four, which time the CLI's pool, `tpdf render --jobs`, on
1, 2, 4 and 8 jobs over one input, so that how the pool scales is a ratio
between operations of one run. Their figures include the process, and
`tpdf` has no library half.
`benches/support/cli.rs` runs this revision's binary from beside the bench
binary's profile directory, one child per call, its output kept in the
operation's own directory and its exit status checked every time, and the
operation's assertions read what that child wrote, after its runs. It is the
one file under `benches/` that `cargo xtask oracles` lets start a program:
the child is this repository's own, so it hosts and adjudicates nothing. A
timed run needs the release `tpdf` built before it measures, and
`cargo test --benches` needs a debug one built first.

**An operation is defined when criterion first runs it.** The harness runs an
operation's definition — what it builds and checks, its routine — the first
time criterion calls the routine, outside the time criterion takes, and keeps
what it built; an operation criterion leaves out is never defined. So a
binary started for one operation, `--exact <group>/<name>` as the paired run
starts each, builds that operation's inputs and nothing for the rest of its
group, and `--list` builds nothing. Criterion takes the figure it reports a
rate in before the definition runs, so the harness hands it the one the
group's list in `baseline.json` commits, and refuses an operation whose own
declaration is not that figure, outside the planning baseline's build.

**Every build of a pair is timed on the head's inputs.** Each group file
takes every input through `benches/support/inputs.rs`' store. With
`TINKER_BENCH_FIXTURES` naming a directory, an input, generated or committed,
is read from the file of its name there, and written there first when the
directory holds none, whole or not at all; a render's bitmap crosses as its
pixels and a set of files as one bundle. A relative path is refused, since
each checkout's processes would resolve it in their own directory. The run
sets it, to a directory that starts empty, for every process of every build,
and the head's process of each operation runs first, so the base and the
planning baseline read the bytes the head wrote rather than writing their
own, and a process writes the inputs its operation reads and no other's.
The calibration operation's input stays each build's own, since its frozen
file generates it itself. Without the variable an input is generated in
memory, and a file a child reads is written into `bench-fixtures/` under the
directory cargo gives the bench target. `tests/bench_inputs.rs` reaches the
store by path and holds an input written into a directory once and read back
by every later call.

**A bench binary also counts.** Started with
`TINKER_BENCH_COUNT=<group>/<name>` and `TINKER_BENCH_ITERATIONS=<n>`, it
builds that operation's inputs alone, runs its routine exactly `n` times
outside criterion's sampler and prints `count-mode: RAN <group>/<name> <n>`;
an operation of another group, or an iteration count missing or no number,
fails by name. It reads no clock, so nothing timed reaches an instruction
count of the process, and a run of `n + 1` less a run of `n` is one
iteration's. Every operation is declared through the harness's
`Group::operation`, or through `Group::calibrated`, which declares through it
outside the planning baseline's build, so each runs both ways, and
`cargo xtask baseline` refuses
a group file that reaches criterion's own `bench_function` or
`benchmark_group` — all but `calibration.rs`, which is frozen and runs under
criterion alone; its input and call are the text group's
`text of 5 dense pages`, which count mode runs.

**`cargo xtask bench-count` holds a head's instruction counts to its
base's.** It counts one group, `--group NAME`, or a part of one,
`--part K/N`: every N-th operation of the head's list from the K-th. The head
is `--head REV`, `HEAD` unless named, and the base `--base REV`, the head's
parent unless named, so a change is counted against its parent. Each side is
checked out as a detached worktree of the repository under
`target/bench-count/<commit>` and builds its release `tpdf` and the group's
release bench binary there, into a target directory of its own, so a CLI
operation runs its own side's `tpdf`. The checkouts and their builds stay, so
the next part of one pair builds nothing again, and `git worktree remove`
takes one out; a directory there that is not its commit's checkout is
refused, and so is a checkout whose bench harness has no count mode, before
anything is built.

Every operation of the part runs in count mode under cachegrind — with
`--cache-sim=no`, its own output file discarded, and every child traced so
that a CLI operation's `tpdf` counts with the bench, each process writing a
log of its own — at one iteration and at two, on each side. A run's count is
the sum of its logs' `I refs`, and the second run less the first is one warm
iteration's instructions, start-up and the inputs' construction cancelled;
`TINKER_BENCH_FIXTURES` is cleared for the runs, so that both build the same
inputs. A run that does not exit 0 and print its `RAN` line, a process whose
log holds no count, and a warm iteration that counts nothing each fail the
task by name.

Each operation's line is printed as it is counted: the change in a call's
instructions — each side's warm iteration over the `calls` its own list gives
the operation, so a change to `calls` moves neither side — then the head's
warm iteration and the base's. A head more than 1 % above its base fails, and
a fall is printed and never failed, one-sided like every gate here. An
operation the base's list does not hold is new at the head: counted, printed
and not judged. One the head's list no longer holds is named and not counted.
`calibration` has no count mode, and the task refuses it: its input and call
are the text group's `text of 5 dense pages`. cachegrind runs on Linux, where
the Linux leg's container needs it installed, and is in the dev-tooling
exemption ([architecture](architecture.md)): it executes the bench and judges
no document. The task is the one that starts it, with `git` and `cargo` to
check out and build this repository's own code, so `xtask/src/count.rs` has
its row in the `SPAWNERS` list `cargo xtask oracles` holds, with that reason.

Run 41204196780 counted all 141 operations of the set, a share at a time,
each share twice at one revision, which is what a base and a head cost, so a
share's time is the time of one pair:

| Job | Operations | Seconds |
| --- | ---: | ---: |
| `open-1-of-6` | 1 | 7 398 |
| `open-2-of-6` | 1 | 2 |
| `open-3-of-6` | 1 | 10 286 |
| `open-4-of-6` | 1 | 305 |
| `open-5-of-6` | 1 | 5 043 |
| `open-6-of-6` | 1 | 6 167 |
| `text` | 13 | 11 713 |
| `render` | 34 | 12 096 |
| `write-1-of-5` | 4 | 111 |
| `write-2-of-5` | 4 | 7 732 |
| `write-3-of-5` | 3 | 213 |
| `write-4-of-5` | 3 | 7 453 |
| `write-5-of-5` | 3 | 5 048 |
| `layout` | 40 | 3 836 |
| `analysis` | 8 | 746 |
| `encode` | 4 | 185 |
| `decode` | 9 | 2 301 |
| `sign` | 3 | 61 |
| `fonts` | 7 | 168 |

The longest job is `render`'s, 12 096 s, so a gate that counts a base and a
head runs at least that long. Most of a slow operation's time is what its
process does besides its iterations, which the counter counts as well:
`open/open and count the 23 MB image file` takes 10 286 s and counts
9 340 950 instructions an iteration, and `open/open the AES-256 file`
7 398 s for 197 966 766.

The two counts of 123 of the 141 operations differ, so a count is not a
property of the code and the input alone. Those of two differ by more than
1 %, more than a 1 % rule can resolve, and those of four others did in
run 41068883186, which counted 138 of the set; which operations pass 1 %
moves from one run to the next:

| Operation | 41068883186 | 41204196780 |
| --- | ---: | ---: |
| `layout/render every page of pandoc-book-cover.epub at 72 dpi` | 0.6143 % | 1.0783 % |
| `layout/render every page of pandoc-plates.epub at 72 dpi` | 0.4085 % | 1.0030 % |
| `layout/render every page of pandoc-book-epub2.epub at 72 dpi` | 3.1337 % | 0.2312 % |
| `analysis/PDF-X validation of 100 pages` | 2.8814 % | 0.6377 % |
| `layout/paginate a book at 432x648 pt` | 1.6343 % | 0.0393 % |
| `write/edit an annotation, save, reopen, 1000 pages` | 1.1968 % | 0.2429 % |

They stay in the set, and how a gate holds them is H-78's, for the owner.
Every count of run 41204196780, by the names and inputs the set had
when it ran:

| Operation | First | Second | Spread |
| --- | ---: | ---: | ---: |
| `open/open the AES-256 file` | 197 966 766 | 197 906 657 | 0.0304 % |
| `open/open a 3-page document` | 176 118 | 175 803 | 0.1792 % |
| `open/open and count the 23 MB image file` | 9 340 950 | 9 330 481 | 0.1122 % |
| `open/every page's size, 3000 pages` | 25 478 794 176 | 25 391 485 633 | 0.3438 % |
| `open/every page's size, 10000 pages` | 289 763 232 333 | 289 252 206 759 | 0.1767 % |
| `open/open and count the 23 MB image file, CLI` | 59 804 639 | 59 827 177 | 0.0377 % |
| `text/extract a page of text` | 10 352 950 | 10 422 003 | 0.6670 % |
| `text/text of 100 dense pages` | 12 133 559 702 | 12 130 809 276 | 0.0227 % |
| `text/text of 100 standard-14 pages` | 3 397 980 995 | 3 391 177 326 | 0.2006 % |
| `text/text of 1000 small pages` | 3 149 016 771 | 3 153 093 404 | 0.1295 % |
| `text/text of 10000 small pages` | 286 543 894 229 | 286 530 454 644 | 0.0047 % |
| `text/structured JSON of 100 pages` | 22 383 968 608 | 22 341 978 462 | 0.1879 % |
| `text/search a phrase over 100 pages from scratch` | 12 415 435 826 | 12 391 319 347 | 0.1946 % |
| `text/search 100 pages' prebuilt text` | 1 443 023 290 | 1 446 255 702 | 0.2240 % |
| `text/search 100 pages' prebuilt text, whole words, unaccented` | 1 953 486 979 | 1 954 466 524 | 0.0501 % |
| `text/text of 100 dense pages, CLI` | 12 152 123 179 | 12 125 236 567 | 0.2217 % |
| `text/text of 1000 small pages, CLI` | 3 161 798 593 | 3 161 632 197 | 0.0053 % |
| `text/text of 10000 small pages, CLI` | 288 545 212 957 | 288 536 266 202 | 0.0031 % |
| `text/structured JSON of 100 pages, CLI` | 22 411 107 172 | 22 411 001 296 | 0.0005 % |
| `render/render text at 150 dpi` | 71 177 584 | 71 282 942 | 0.1480 % |
| `render/render a full-page axial shading at 150 dpi` | 2 230 683 155 | 2 230 684 648 | 0.0001 % |
| `render/fill 300 anti-aliased paths at 150 dpi` | 579 745 308 | 579 743 765 | 0.0003 % |
| `render/the text page at 72 dpi` | 2 421 927 879 | 2 423 584 042 | 0.0684 % |
| `render/the text page at 150 dpi` | 1 637 078 434 | 1 639 072 951 | 0.1218 % |
| `render/the text page at 300 dpi` | 1 626 713 015 | 1 628 093 143 | 0.0848 % |
| `render/record the text page's display list` | 2 545 820 755 | 2 538 659 592 | 0.2821 % |
| `render/5000 paths at 150 dpi` | 17 598 511 995 | 17 598 511 458 | 0.0000 % |
| `render/5000 paths at 300 dpi` | 52 467 795 487 | 52 467 796 105 | 0.0000 % |
| `render/the 2000 stroked curves alone at 150 dpi` | 10 459 943 615 | 10 459 943 285 | 0.0000 % |
| `render/a full-page axial shading at 300 dpi` | 8 920 009 722 | 8 920 008 822 | 0.0000 % |
| `render/the mesh shadings fixture at 150 dpi` | 1 517 686 654 | 1 517 684 172 | 0.0002 % |
| `render/the mesh shadings fixture at 300 dpi` | 1 743 741 551 | 1 743 739 419 | 0.0001 % |
| `render/the transparency fixture at 150 dpi` | 636 838 710 | 636 837 821 | 0.0001 % |
| `render/a page tiled by one pattern fill at 150 dpi` | 761 663 104 | 761 662 673 | 0.0001 % |
| `render/a page tiled by 50 pattern fills at 150 dpi` | 1 191 258 315 | 1 191 258 412 | 0.0000 % |
| `render/the 300-dpi text page in 4 bands on 4 threads` | 6 180 633 910 | 6 185 998 093 | 0.0868 % |
| `render/100 text pages at 150 dpi on 1 thread` | 81 915 427 936 | 81 819 195 681 | 0.1176 % |
| `render/100 text pages at 150 dpi on 4 threads` | 82 019 897 396 | 82 048 181 180 | 0.0345 % |
| `render/the annotated text page at 150 dpi` | 1 650 715 035 | 1 649 568 209 | 0.0695 % |
| `render/re-render a changed 100x100 pt annotation` | 2 522 825 341 | 2 525 396 795 | 0.1019 % |
| `render/replay the Flate RGB image page at 150 dpi` | 4 053 692 989 | 4 053 694 118 | 0.0000 % |
| `render/an ICC-based CMYK image to sRGB at 300 dpi` | 24 812 416 640 | 24 812 417 161 | 0.0000 % |
| `render/the JPEG page at 72 dpi` | 8 375 931 247 | 8 375 930 782 | 0.0000 % |
| `render/the JPEG page at 300 dpi` | 18 796 333 308 | 18 796 332 859 | 0.0000 % |
| `render/a second render of the JPEG page at 300 dpi` | 27 173 043 386 | 27 173 044 484 | 0.0000 % |
| `render/the JPEG 2000 page at 72 dpi` | 27 143 758 867 | 27 143 757 636 | 0.0000 % |
| `render/the G4 scan page at 150 dpi` | 7 410 916 878 | 7 411 003 742 | 0.0012 % |
| `render/the JBIG2 scan page at 150 dpi` | 8 651 714 150 | 8 651 716 095 | 0.0000 % |
| `render/the Flate RGB image page at 72 dpi` | 11 697 078 589 | 11 697 077 403 | 0.0000 % |
| `render/the text page to PNG at 150 dpi, CLI` | 3 685 211 958 | 3 685 033 082 | 0.0049 % |
| `render/the text page to PNG at 300 dpi, CLI` | 12 084 637 446 | 12 084 635 979 | 0.0000 % |
| `render/20 pages to PNG at 150 dpi, 1 job, CLI` | 73 638 978 413 | 73 645 074 132 | 0.0083 % |
| `render/20 pages to PNG at 150 dpi, 4 jobs, CLI` | 73 635 197 969 | 73 628 993 252 | 0.0084 % |
| `write/rewrite a document` | 843 939 | 844 334 | 0.0468 % |
| `write/rewrite 100 pages` | 4 448 161 | 4 445 155 | 0.0676 % |
| `write/rewrite 1000 pages` | 44 188 742 | 44 188 755 | 0.0000 % |
| `write/rewrite 3000 pages` | 132 884 356 | 132 922 175 | 0.0285 % |
| `write/rewrite 100 pages into object streams` | 44 100 984 | 43 910 316 | 0.4342 % |
| `write/rewrite 100 pages linearized` | 72 985 462 | 72 790 061 | 0.2684 % |
| `write/subset-on-save of 100 pages` | 14 802 503 152 | 14 830 250 367 | 0.1874 % |
| `write/merge 4 x 100 pages` | 9 001 394 444 | 9 001 284 989 | 0.0012 % |
| `write/AES-256-CBC decrypt 23 MB` | 6 592 853 140 | 6 592 858 161 | 0.0001 % |
| `write/AES-256-CBC encrypt 23 MB` | 5 753 336 887 | 5 753 341 009 | 0.0001 % |
| `write/edit an annotation, save, reopen, 1000 pages` | 63 547 315 | 63 393 357 | 0.2429 % |
| `write/edit an annotation, save, reopen, 10 pages` | 287 137 | 289 265 | 0.7411 % |
| `write/rewrite 100 pages, CLI` | 42 656 851 | 42 641 876 | 0.0351 % |
| `write/rewrite 1000 pages, CLI` | 99 394 956 | 99 349 085 | 0.0462 % |
| `write/subset-on-save of 100 pages, CLI` | 14 931 033 161 | 14 930 996 889 | 0.0002 % |
| `write/merge 4 x 100 pages, CLI` | 9 017 630 789 | 9 017 298 902 | 0.0037 % |
| `write/encrypt the 23 MB image file, CLI` | 7 670 137 790 | 7 670 130 613 | 0.0001 % |
| `layout/paginate a book at 432x648 pt` | 182 327 799 | 182 399 428 | 0.0393 % |
| `layout/open and paginate calibre-book-cover.epub at 432x648 pt` | 184 154 232 | 184 457 926 | 0.1649 % |
| `layout/open and paginate calibre-book-nocover.epub at 432x648 pt` | 180 801 447 | 180 811 884 | 0.0058 % |
| `layout/open and paginate calibre-embedded-font.epub at 432x648 pt` | 619 487 869 | 619 808 043 | 0.0517 % |
| `layout/open and paginate kcc-fixed-layout.epub at 432x648 pt` | 32 057 574 | 32 060 276 | 0.0084 % |
| `layout/open and paginate pandoc-book-cover.epub at 432x648 pt` | 729 106 300 | 732 860 531 | 0.5149 % |
| `layout/open and paginate pandoc-book-epub2.epub at 432x648 pt` | 730 436 647 | 731 497 159 | 0.1452 % |
| `layout/open and paginate pandoc-book-nocover.epub at 432x648 pt` | 717 831 424 | 717 091 306 | 0.1032 % |
| `layout/open and paginate pandoc-embedded-font.epub at 432x648 pt` | 553 719 791 | 552 730 273 | 0.1790 % |
| `layout/open and paginate pandoc-plates.epub at 432x648 pt` | 201 586 452 | 201 854 265 | 0.1329 % |
| `layout/render every page of calibre-book-cover.epub at 72 dpi` | 57 837 519 | 57 638 844 | 0.3447 % |
| `layout/render every page of calibre-book-nocover.epub at 72 dpi` | 49 414 776 | 49 616 229 | 0.4077 % |
| `layout/render every page of calibre-embedded-font.epub at 72 dpi` | 125 546 815 | 125 928 418 | 0.3040 % |
| `layout/render every page of kcc-fixed-layout.epub at 72 dpi` | 38 759 955 | 38 766 806 | 0.0177 % |
| `layout/render every page of pandoc-book-cover.epub at 72 dpi` | 77 139 095 | 76 316 209 | 1.0783 % |
| `layout/render every page of pandoc-book-epub2.epub at 72 dpi` | 76 033 744 | 75 858 342 | 0.2312 % |
| `layout/render every page of pandoc-book-nocover.epub at 72 dpi` | 67 392 626 | 68 057 513 | 0.9866 % |
| `layout/render every page of pandoc-embedded-font.epub at 72 dpi` | 166 652 786 | 166 448 026 | 0.1230 % |
| `layout/render every page of pandoc-plates.epub at 72 dpi` | 66 667 158 | 67 335 856 | 1.0030 % |
| `layout/open and paginate the 1000-chapter book` | 50 540 442 094 | 50 533 274 606 | 0.0142 % |
| `layout/render the 1000-chapter book at 72 dpi` | 62 547 934 607 | 62 771 429 880 | 0.3573 % |
| `layout/open and paginate the 100-chapter book` | 5 082 630 152 | 5 081 059 205 | 0.0309 % |
| `layout/render the 100-chapter book at 72 dpi` | 6 230 263 077 | 6 220 358 505 | 0.1592 % |
| `layout/the 20-page HTML report to PDF` | 293 114 678 | 292 598 153 | 0.1765 % |
| `layout/the 20-page Markdown document to PDF` | 333 027 377 | 332 310 638 | 0.2157 % |
| `layout/open each SVG fixture, render page 1` | 295 351 307 | 295 367 313 | 0.0054 % |
| `layout/render each SVG fixture's synthesised PDF` | 255 194 271 | 255 277 307 | 0.0325 % |
| `layout/open each XPS fixture, render page 1` | 2 418 397 662 | 2 421 058 865 | 0.1100 % |
| `layout/render each XPS fixture's synthesised PDF` | 1 949 245 698 | 1 949 435 305 | 0.0097 % |
| `layout/open each CBZ fixture, render page 1` | 123 292 670 | 123 107 036 | 0.1508 % |
| `layout/render each CBZ fixture's synthesised PDF` | 91 370 607 | 90 639 093 | 0.8071 % |
| `layout/render every page of calibre-book-cover.epub at 72 dpi, CLI` | 1 112 950 498 | 1 113 867 554 | 0.0824 % |
| `layout/render every page of calibre-book-nocover.epub at 72 dpi, CLI` | 902 696 935 | 902 619 359 | 0.0086 % |
| `layout/render every page of calibre-embedded-font.epub at 72 dpi, CLI` | 813 742 088 | 814 048 646 | 0.0377 % |
| `layout/render every page of kcc-fixed-layout.epub at 72 dpi, CLI` | 75 218 749 | 75 226 973 | 0.0109 % |
| `layout/render every page of pandoc-book-cover.epub at 72 dpi, CLI` | 1 677 733 752 | 1 675 200 111 | 0.1512 % |
| `layout/render every page of pandoc-book-epub2.epub at 72 dpi, CLI` | 1 677 397 903 | 1 675 493 151 | 0.1137 % |
| `layout/render every page of pandoc-book-nocover.epub at 72 dpi, CLI` | 1 464 681 462 | 1 463 614 935 | 0.0729 % |
| `layout/render every page of pandoc-embedded-font.epub at 72 dpi, CLI` | 1 290 930 847 | 1 290 036 523 | 0.0693 % |
| `layout/render every page of pandoc-plates.epub at 72 dpi, CLI` | 1 148 557 059 | 1 148 387 321 | 0.0148 % |
| `analysis/text of the 100 report pages` | 10 633 534 182 | 10 633 522 395 | 0.0001 % |
| `analysis/redact a rectangle on each of 100 pages` | 1 260 488 481 | 1 254 285 760 | 0.4945 % |
| `analysis/reading order inferred over 100 pages` | 22 631 369 843 | 22 581 444 575 | 0.2211 % |
| `analysis/tables inferred over 100 pages` | 11 268 909 162 | 11 279 185 134 | 0.0912 % |
| `analysis/text of the 100 claiming pages` | 10 637 092 209 | 10 653 885 095 | 0.1579 % |
| `analysis/PDF-A validation of 100 pages` | 886 726 299 | 894 822 431 | 0.9130 % |
| `analysis/PDF-UA validation of 100 pages` | 754 719 301 | 759 678 700 | 0.6571 % |
| `analysis/PDF-X validation of 100 pages` | 370 702 216 | 368 353 229 | 0.6377 % |
| `encode/PNG of the 1240x1755 text page` | 2 868 954 230 | 2 868 956 058 | 0.0001 % |
| `encode/PNG of the near-blank page` | 3 486 422 576 | 3 486 422 367 | 0.0000 % |
| `encode/deflate page-like data` | 1 646 042 412 | 1 646 042 412 | 0.0000 % |
| `encode/deflate photo-like data` | 1 578 560 042 | 1 578 560 042 | 0.0000 % |
| `decode/inflate page-like data` | 204 987 824 | 204 987 824 | 0.0000 % |
| `decode/inflate near-incompressible data` | 394 558 041 | 394 558 041 | 0.0000 % |
| `decode/LZW decode page-like data` | 165 836 076 | 165 866 150 | 0.0181 % |
| `decode/baseline JPEG 2480x3508` | 6 643 680 847 | 6 643 680 847 | 0.0000 % |
| `decode/progressive JPEG 2480x3508` | 7 314 140 717 | 7 314 140 717 | 0.0000 % |
| `decode/irreversible JPEG 2000 2480x3508` | 25 095 196 363 | 25 095 196 363 | 0.0000 % |
| `decode/reversible JPEG 2000 2480x3508` | 65 490 751 249 | 65 490 751 249 | 0.0000 % |
| `decode/decode the JBIG2 generic page` | 1 733 557 255 | 1 733 769 914 | 0.0123 % |
| `decode/decode the CCITT G4 page` | 492 030 759 | 492 030 415 | 0.0001 % |
| `sign/RSA-2048 private-key operation` | 1 467 364 232 | 1 467 364 232 | 0.0000 % |
| `sign/verify an RSA-2048 signature over 10 MB` | 630 663 743 | 630 663 743 | 0.0000 % |
| `sign/SHA-256 of 10 MB` | 629 848 693 | 629 848 693 | 0.0000 % |
| `fonts/shape 1000 characters of Latin` | 709 864 010 | 709 864 010 | 0.0000 % |
| `fonts/shape 1000 characters of Arabic` | 2 547 846 611 | 2 547 846 611 | 0.0000 % |
| `fonts/shape 1000 characters of Devanagari` | 7 461 150 223 | 7 461 150 223 | 0.0000 % |
| `fonts/outlines of Liberation Serif Regular` | 237 622 784 | 237 622 784 | 0.0000 % |
| `fonts/outlines of Liberation Sans Regular` | 162 110 765 | 162 110 765 | 0.0000 % |
| `fonts/outlines of Liberation Mono Regular` | 238 967 609 | 238 967 609 | 0.0000 % |
| `fonts/outlines of every committed CFF program` | 665 864 736 | 665 864 736 | 0.0000 % |

**The comparison is `cargo xtask bench-check --machine NAME`, and it exists
because criterion's own cannot fail.** `cargo bench -- --baseline` compares
perfectly well and **never exits non-zero when it loses**: the only failure path
in 0.5.1 is a *missing* baseline directory, and a regression prints
"Performance has regressed" and exits 0. `bench-check` reads criterion's own
`estimates.json` — its numbers, not a second timing — and holds each operation
to a committed figure and a band in `crates/tinker-pdf/benches/baseline.json`.

A band has to be above the machine's own swing or it fails on the machine
rather than on the code, so `bench-check` refuses an entry that carries no
measured swing.

`bench-check` prints each operation's throughput beside its mean, in the unit
its list in `baseline.json` commits it to: pages a second, megapixels or
megabytes a second, or outlines a second. With `--sizes` it also holds every
head-side operation to the 20-300 ms window the set is sized to, on the
machine it runs on, which is where the window is a fact: an operation outside
it fails unless its entry names it outside the window by the owner's word
(`outside_window`), which is reported; the planning baseline's side of
a factor may run longer. `--sizes report` lists every operation outside the
window with its mean and fails on none.

**Some relations are held inside one run.** A relation whose entry is marked
`in_one_run` is a floor `bench-check --machine NAME` holds on every run,
whatever the machine and whether or not it has an entry, from that run's own
estimates and with no band: its operation's per-call time over its other's,
against its bound, each ratio printed beside its floor. They are MS-09's
scaling floors — the `threads` group's 2 and 4 jobs at least 1.9 and 3.6
times as fast as one job, its 8 jobs at least 0.95 times as fast as 4 on a
run that recorded four logical CPUs, and the render group's four bands at
least 2.6 times as fast as its one-thread 300-dpi page — and only the owner
lowers one (H-78). The paired run's rounds leave them to this check. When it
measures a group, the harness writes `host.json` into the group's directory
beside the estimates criterion writes there: the logical CPUs
`std::thread::available_parallelism` counts and the CPU as the OS names it,
`/proc/cpuinfo`'s first model name or Windows' `PROCESSOR_IDENTIFIER`, and
`bench-check` prints both beside the group's relations. A relation that names
`cpus` is armed only on a run whose group recorded that many: on another
count it is reported as not armed, and in a run that recorded none it fails.
A relation of a group that ran fails when an estimate it reads is missing;
one whose group did not run at all, no estimate of its and no `host.json`,
is reported.

Criterion 0.5.1 on Windows trims each part of an operation's directory at
its end and lowercases it, so a name holding a capital is recorded under
another key there; `bench-check` reads each directory, in a plain run and in
a pair's rounds alike, as the listed id it was made from.

### The named machine is `ubuntu-latest`

The committed entry is `ubuntu-latest`'s, the hosted runner its fifteen runs
landed on, and `bench-check --machine ubuntu-latest` holds a run to it. Its
figures are criterion's own `mean.point_estimate`, read out of each run's
`estimates.json` — nothing re-times anything, and the machine hosts this code
rather than judging it (ruling 13). `baseline.json` lists the runs its figures
were taken from. A local machine is another machine: with no entry of its own
it is reported and not failed, and its own entry, measured as this one was,
is what holds it.

**Three choices in the entry, and the reason for each.**

*The recorded figure is the fastest of fifteen runs, not their average.* The
comparison is one-sided — faster is never a failure — so the bar worth
recording is the best the machine has ever done, and a verdict then reads as
*how much slower than that*. A middle figure would raise the bar by whatever
the slow half of the fleet contributed and slacken the gate by the same
amount, for nothing.

*The band is per operation.* One band would have to clear the worst spread of
any operation, and would hold a steady operation to a noisy one's slack: a
doubling of the steadiest would pass without a word. `ratchet.json` bands each
corpus's peak separately rather than banding all of them on the worst, and
this is the same argument. `baseline.json` also carries a machine-level pair:
the widest swing any of its operations showed, and a ceiling no operation's
band may exceed.

*Each band is its operation's spread, half again as wide, rounded up to the
next 5 %.* Half again because a spread measured over fifteen runs is a lower
bound that has not converged, and it grows with the sample, so a band set just
above the observation would fail on the machine rather than on the code —
which is the thing `bench-check` refuses an entry for. These bands are a
floor, and a later measurement may have to raise them.

**`ubuntu-latest` is a fleet and not a machine.** Runs of one revision fall
into groups on hardware the log does not name, and much of a spread is that
split rather than noise, so each group's run records its host beside its
figures, in the group's `host.json`, so that a measurement can attribute the
split rather than band it.

**A pair is judged on its rounds.** `cargo xtask bench-check --paired BASE
HEAD` holds a head's rounds to a base's, timing nothing a second time. Each
side is a directory of ten rounds, `round-01` to `round-10`, each the
`CRITERION_HOME` one round of one build wrote, with that build's committed
`baseline.json` beside them. An operation's figure for a round is its
`estimates.json` mean over the `calls` its own side's list gives it, so a
change to `calls` moves neither side, and its `sample.json` must hold ten
samples taken flat. Each round's ratio is the head's figure over the
base's; the operation fails when the median of the ten is above 1 + its
band and at least eight of the ten rounds are slower. The band keeps two
builds of one revision green, and the sign test asks that a slowdown show in
eight rounds of ten rather than in a few far ones. An operation one side
alone timed is reported, not judged; a missing round, or a round sampled
otherwise, is refused; and an operation whose list carries no `paired` band
is reported, never failed.

**Each operation carries its gate against the planning baseline.** Tier P
states every target against `61adcde`, the planning baseline, and its Gate
column says how each is checked ([roadmap/P.md](roadmap/P.md)); each
operation's entry in `baseline.json` carries that as its `gate`. A `floor`
holds the head's per-call time over the planning baseline's, round by round,
to 1 ÷ the floor: floors start at 1, a P row raises its operations' to the
column's factors in the commit that closes it, and only the owner lowers one
(H-78). `held` is a floor of 1 that no row raises. An operation the planning
baseline lacks is calibrated: its target, in milliseconds a call
(`calibrated_ms`) or as the rate tier P states, in its unit a second
(`calibrated_per_second`), is held as a fraction of the planning baseline's
figure for the calibration operation in the same round — the file names that
operation once, ahead of its groups, with `planning_ms`, its figure on the
planning machine — so a target reads as the same fraction on whatever machine
the run is on. A relation holds two of the head's operations of one group in
one round: one's time `at_most` so many times the other's, or its
`speedup_at_least` so many times over the other; one marked `in_one_run` is a
plain run's instead, held inside each run as above. An operation tier P's table
does not list, such as the in-process twin of a CLI-marked one, sits at a
floor of 1.

`cargo xtask bench-check --paired --planning PLANNING HEAD` reads the planning
baseline's rounds and the head's as `--paired` reads a base's and a head's,
and holds every gate by the regression pair's rule with the gate's bound in
place of 1: a gate fails when the median of its ten ratios is beyond the bound
by more than the operation's band and at least eight of the rounds are beyond
the bound, a relation taking the wider of its two operations' bands. Tier P's
own reading — the median against the bound alone — is printed beside it: with
the two builds equally fast, a floor of 1 read that way fails about one run in
two. A gate whose operations carry no band is reported, never failed. A gate
the rounds cannot judge is refused rather than reported: an operation the
head's lists hold and the head did not time, a floor or a held operation the
planning baseline did not time, and a calibrated one beside which the planning
baseline timed no calibration.

**The planning baseline builds the head's bench files.** `61adcde` is checked
out beside the head, its own `crates/tinker-pdf/benches/` replaced by the
head's and its `[[bench]]` entry by the head's entries, and built under
`--cfg tinker_planning_baseline`, which the head's `crates/tinker-pdf/Cargo.toml`
declares to `check-cfg` and the planning build declares on its own command
line. Under the `cfg` an operation's body calls what `61adcde`'s facade has:
where the head calls what that revision lacks, an item marked with the `cfg`
computes the same thing from what it has. An operation the planning baseline
lacks altogether is declared through `Group::calibrated`, which that build
leaves out, and its gate holds it against the calibration operation instead;
an operation whose gate names `paired_with` is one the planning baseline
cannot express, and its arm there runs the named operation of its group under
its id. `cargo xtask baseline` holds each declaration to its gate: a
calibrated operation is declared `calibrated`, every other through
`operation`, and a pairing names another operation of its group that has a
side of its own. The build sets no `-D warnings`, since a lint a later stable
adds must not fail a frozen revision, and that revision's own `tpdf` builds in
the same target directory under the same flags, for the CLI-marked
operations. Nothing here is a spawner: the checkout and the builds are the
local run's own commands.

```sh
export TINKER_BENCH_FIXTURES="$(mktemp -d)"
cargo build -p tpdf
cargo test -p tinker-pdf --benches
git worktree add --detach ../planning 61adcde
rm -r ../planning/crates/tinker-pdf/benches
cp -r crates/tinker-pdf/benches ../planning/crates/tinker-pdf/benches
sed -i '/^\[\[bench\]\]$/,$d' ../planning/crates/tinker-pdf/Cargo.toml
sed -n '/^\[\[bench\]\]$/,$p' crates/tinker-pdf/Cargo.toml >> ../planning/crates/tinker-pdf/Cargo.toml
cd ../planning
export RUSTFLAGS="--cfg tinker_planning_baseline --check-cfg cfg(tinker_planning_baseline)"
cargo build -p tpdf
cargo test -p tinker-pdf --benches
```

The two `sed` lines rely on the `[[bench]]` entries being the last of each
manifest. The head's `cargo test --benches` runs every operation once and
writes every input into the directory; the planning baseline's then runs
every operation with a side on it once, beside a debug `tpdf`, on those
bytes, and writes none of its own. A timed run builds both with
`--release`.

**A pinned machine names its host.** An entry for one host held still
carries `cpu`, that host's `/proc/cpuinfo` model name, and `held`, the
frequency and core isolation it is held at, and `bench-check --machine NAME`
reads this host's model name from `/proc/cpuinfo` and refuses to
compare when it is not the entry's: a run that lands on another host is
refused by name rather than judged against figures it did not set. A fleet
names no model, so `ubuntu-latest` compares on any host.

**What this does not catch.** The widest band passes a change that makes its
operation half again slower in silence; a several-fold regression would not.
This is a ratchet against the catastrophic and not against the incremental,
and the finer instrument is criterion's own `--baseline` between two revisions
on one machine, which needs no band because it changes nothing but the code.

**The entry is checked, not just written.** `cargo xtask baseline` — part of
`cargo run -p xtask -- check`, so it fails a build — holds the files that carry
each group's list of operations to each other: a `[[bench]]` declares each
group file, the file defines the group's operations, and `baseline.json` lists
them, in the file's order and each with its unit and its gate, and records a
figure for each or names it unmeasured. It refuses an operation with no gate, a
gate outside the format — a floor under 1 among them, and a relation armed on
a CPU count that is not held inside one run — a relation naming no
other operation of its group, and a calibrated target with no calibration
operation the lists hold to be a fraction of. It refuses a name criterion would
record under another key — one holding any of the ten characters it writes as
`_`, or longer than the 64 bytes it keeps — since a name it rewrites is
reported as new and never held to its band. Every band in the committed file is
pushed against from both sides by a unit test, because a band nobody has ever
crossed is a number nobody has checked.

## Counted work, guarded

A speed claim that can hold on a shared runner and under every target is a
relation over counted work, never a time: the `Stats` a door hands back
([performance](design/performance.md)), held to a bound or compared at two
sizes of one input. `crates/tinker-pdf/tests/guards.rs` is where those guards
live, and it holds what they stand on.

- **The roster**: every guard, A1 to A10 — its clause, its test, the file the
  test is in, and the row that owns it — landed or owed.
  `every_guard_is_landed_or_owed` reads each file it names, built in through
  `include_str!` because the wasm leg has no filesystem, and fails when a
  landed test is missing or ignored there, or an owed one is already there: a
  guard is added in the commit of the row that makes it hold, which flips its
  entry. `the_roster_tells_a_running_test_from_every_other_case` holds that
  reading itself.
- **Allocations a page**, A10's count, which no tally sees:
  `guard_allocations.rs` installs the counting allocator in
  `guard_support/mod.rs`, which counts each thread's allocations apart, and
  `allocations_per_page_by_operation` prints what page lookup, text and a
  render at 36 dpi allocate a page over every page of a thousand and of four
  thousand, and their ratios; the figures are what it prints, and A10's
  roster entries follow it ([performance](design/performance.md) states the
  rule they are held to).
- **The generators**, each held to its shape by a test of its own: a balanced
  tree of ten thousand pages, ten kids a node over four levels; documents of
  a thousand and four thousand pages, a line of text each, for comparing one
  operation at two sizes; a page of 5 600 glyphs of 80 distinct codes; four
  documents of a hundred pages to merge; two A4 pages for the PNG path,
  seventy lines of text and one; a hundred pages each drawing one embedded
  face with a `/ToUnicode`, to save; and a page showing one form three times
  and one Type 3 glyph fifty times, whose form draws the page's face and an
  image in the profile an image of the page's shares, to render, extract text
  from and list the images of. Each is written object by object in memory,
  and every glyph is drawn in an embedded face, so both builds count alike and
  nothing is read from disk.
- **The pins**: `the_tally_counts_the_same_on_every_target` holds a render's,
  a text extraction's, a save's and a page lookup's counts over one fixed page
  to committed values — only the counts the page decides, never how a cache
  or a walk does the work — so a target that counted differently fails. A pin
  a fix has to move is a finding the owner rules on, as a moved fingerprint
  is.
- **No clock**: `the_guard_suite_reads_no_clock` fails on a clock in any
  source the suite is built from or names.
- **On three targets**: CI's tests run `guards.rs` and
  `guard_allocations.rs` on Linux and Windows in both builds — the default in
  `cargo test --workspace`, `bundled-fonts` in the facade's own run of that
  build — and they run on `wasm32-wasip1` under wasmtime locally.
  `every_guard_is_landed_or_owed` and `the_tally_counts_the_same_on_every_target`
  are in the determinism roster ([determinism](features/determinism.md)), and a
  guard a row lands later joins the roster in that row's commit.

## Peak memory, a process a case

A peak resident set belongs to a whole process, so
`crates/tinker-pdf/tests/peak_memory.rs` measures each of its seven operations
in a process of its own: a page rendered at 300 dpi to PNG, the JPEG page at
300 dpi, the JPEG 2000 page at 72, twenty pages at 150 dpi to PNG on four
scoped threads, the text of 10 000 small pages, a merge of four 100-page files
and the open of the 23 MB image file. The test binary re-executes itself once a
case. The parent writes each case's input from the benchmark set's own
generators (`benches/support/inputs.rs`, reached by path); the child reads it
once, into the buffer it opens, performs the one operation and prints the
`VmHWM` the kernel tracked since exec, read as `tpdf`'s `peak_bytes` reads it,
so a figure holds the operation and its input and never the generator.

The open case's child reads its file as `tpdf info` does instead, through the
CLI's own open (`tools/tpdf/src/source.rs`, reached by path): on demand, the
document kept streamed only when it answers as a buffered open of the same
bytes would, and the pages counted. It asserts that the open kept the document
streamed, that every read was made, and that it counts the pages the parent's
buffered open of the bytes counted, which the parent writes beside the file.
Its figure is then the child's resident floor and what those reads kept,
rather than the file. `the_open_case_reads_its_file_on_demand` runs that case
alone and holds its peak below the file's length, which a whole read could not
be, printing its line and a `RAN` line:

```sh
cargo test --release -p tinker-pdf --test peak_memory -- --ignored --nocapture the_open_case_reads_its_file_on_demand
```

`the_open_case_counts_a_file_it_keeps_streamed` performs the case's operation
in the test's own process, on every target, over a small document.

The four threads draw their twenty pages in waves: thread `k` takes pages `k`,
`k + 4`, …, and each wave starts together and is finished by all four before
the next starts, so the pages alive at once are one wave's whichever thread
finishes first. The case states the allocator it runs under, in its child's
environment: glibc's mmap threshold held at the 128 KiB it starts at
(`MALLOC_MMAP_THRESHOLD_=131072`), since a threshold left to rise with the
order the threads free their buffers decides which of the next buffers a
thread's arena keeps after they are freed, and with it the peak. An eighth
case runs it again with the allocator held to one arena besides
(`MALLOC_ARENA_MAX=1`).

The parent holds each case to its build's figure in `peak_memory.tsv`, the
default and `bundled-fonts` builds each with their own, plus the 2 % the corpus
bars give their peaks, and fails a case the table has no line for. Each figure
is the larger of two runs' peaks on the hosted Linux image, the runs and the
spread between them recorded in the table, so a local run on another machine
reads its peaks beside that machine. A case named `unheld`, by the owner's word
(H-78), is measured and printed and held to nothing: one whose two runs spread
by 2 % or more, or one whose two runs have not been read, as the two threaded
cases' have not under their waves and their allocator.
`TINKER_PEAK_RECORD` prints each case's line for the table instead of holding
it:

```sh
TINKER_PEAK_RECORD=1 cargo test --release -p tinker-pdf --test peak_memory -- --ignored --nocapture
```

It is Linux-only, since the peak is read from `/proc`, and `#[ignore]`d, since
a render at 300 dpi and ten thousand pages of text are a release binary's
work. What it decides with the numbers — the status line read in KiB, the
band, the refusal, the line the record switch prints — and the allocator each
case states are plain functions its own unit tests hold on every target, a
unit test holds the waves on every target with threads, and
`a_child_holding_64_mib_reports_at_least_that` shows that a child's figure
sees what the child holds. The child is this test binary, so the file is a
`SPAWNERS` row: it hosts one case a process and adjudicates nothing.

It runs locally in release and in both builds, holds every case to the
table, and prints its `RAN` line and each case's line.

## The examples, run rather than compiled

`crates/tinker-pdf/examples/` holds the documented end-to-end usage: open,
render, extract, edit, create, convert, parallel. Each runs with no arguments
against a committed fixture, so trying one needs no corpus and no download.

The gates' `examples` gate **runs each of them and reads its output**, which
is the same rule as everywhere else on this page: `cargo clippy --all-targets`
already compiles them, and a program that compiles and prints nothing is
indistinguishable from one that works. The gate asserts content — `the
original, untouched` from the incremental save, `read back page 1 says` from
the writer reading back what it wrote — so an example reduced to an error
message fails it.
