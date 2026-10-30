# Verification

The posture in one sentence: **correctness claims are executable** —
never-panic is a fuzz target that never crashes, determinism is a committed
fingerprint, render quality is a perceptual budget, coverage of the real
world is a ratcheted corpus run, and a claim nothing executes is written
down as a claim.

`cargo test --workspace` runs every crate's unit tests, its integration tests
under `tests/` and its doc tests. The censuses that walk the fetched corpora
are `#[ignore]`d there and run in `corpus.yml`; a test that reads anything else
that cannot be committed prints a `RAN` or `SKIPPED` banner (below).

## Never panic, fuzz-enforced

Ruling 1 makes a fuzz crash a release blocker. cargo-fuzz targets in
`fuzz/fuzz_targets/` cover every input format, each landing in the same change
as its parser, and each has a seed corpus of its own name in `fuzz/corpus/`.

CI runs them twice over. On every push to `main` or `develop` and every pull
request, `ci.yml`'s `fuzz-seeds` job builds every target and mutates for twenty
seconds from each seed corpus (`-max_total_time=20`). The nightly `fuzz.yml`
runs each target longer, in a job of its own — 600 seconds unless a dispatch
names another figure — and attaches the crashing input or the minimised
corpus. It reads its matrix off `fuzz/fuzz_targets/`, so that a target nobody
runs cannot exist. Its schedule runs the default branch, `main`; `develop` is
fuzzed by dispatching it there.

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
control prefix and its signature check — derived from `fuzz/fuzz_targets/`,
`fuzz/corpus/` and `fuzz/Cargo.toml`, and fails if the three disagree or a
target has no seeds. The same audit runs inside `cargo xtask check`, so the
three lists cannot drift apart without a gate going red.

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

`crypt`, `crypt_ciphers`, `form_script`, `layout`, `render_page` and
`signatures` are structured generators: the body is carved into fields rather
than parsed as a format, so every seed reaches the code by construction and
there is no reachability question of this shape to ask. `cos_document`,
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
over `fuzz/`'s own manifest and fails with the compiler's own error, and CI's
`fuzz-targets` job does the same. It is `cargo check` rather than
`cargo fuzz build` because the question is whether the *types* still agree; it
runs on stable and needs no sanitizer, and `fuzz-seeds` builds them for real.

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
`fuzz-seeds` replays each corpus and then mutates from it for the rest of its
twenty seconds, so an input it reports was generated on the runner rather than
committed, and a session that ends clean is a record of what that session
happened to reach. Coverage is uneven by design, too: a hardened password hash
runs a few executions a second where an ASCII filter runs tens of thousands,
and a clean result at the first rate is not the same evidence as one at the
second. A committed seed corpus replayed on stable by an ordinary test is
weaker again, and never counts as a session.

A corpus written by an `#[ignore]`d test in the crate that owns the fixtures —
any `fuzz/corpus/<name>` a crate test calls `fs::write` on — cannot drift from
those fixtures. A corpus with no writer, such as `jbig2`'s, holds inputs a
fuzzer found or a person wrote, replayed by a test (`jbig2_seeds.rs`) and
never regenerated, which is a different guarantee.

`icc_profile`'s first gate is the `acsp` signature at byte 36, so random bytes
reach its parser essentially never. Its seeds include profiles this build
*refuses*, deliberately: `NeedsLut` and `MissingTags` are reached after the
header check, the tag count and the whole `132 + i * 12` table walk, which is
where that format's arithmetic lives. Each seed states which it is and the test
asserts it, so a seed that quietly stopped parsing cannot sit in the corpus
looking like coverage it does not give.

Fuzzers need nightly, so a **hostile-input sweep runs on stable on every
commit**: seeded xorshift mutation of the real fixtures, driven as an
ordinary test (`crates/tinker-pdf/tests/hostile_input.rs`).

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
child process a file, and `corpus.yml` runs it nightly on `main`, and on
`develop` by dispatch, against three committed bars. The first,
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
stopped in. The limit of the signal is worth stating — one unit of work longer
than the stall window is silent for the same reason a hang is, so what the
runner claims honestly is *made no observable progress for half its budget*.

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
is not eligible and is counted as neither. What the semantic tier finds in the
same rewrites — outline counts that disagree with their own trees, backwards
annotation rectangles, malformed `/W` arrays — is listed per file and per rule
in the run's `corpus/report.json`, and belongs to the documents. The pass sees
writer defects nothing else here does, because this engine's own reader
repairs them in silence.

The ratchet: recorded rates may not decrease; improving one updates the
recorded floor in the same change. The floor and the budgets live in-repo,
diffable, so lowering one is the owner's decision (H-78) rather than a drift.

### The fourth axis: relations between two renders

The corpus has no ground truth — nobody here knows what its pages should look
like — but a *relation between two renders of one file* needs none, and it can
be asked of every file at once. `tpdf probe` checks three in-process, on the
first page, and reports **four** judgements — `rotate` is judged twice, against
the budget that decides pass or fail and again against a tighter line that
decides nothing — and `corpus/ratchet.json` records how many files each was
asked of beside how many it held on. What declines a relation for a file is a
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

No test and no CI job in this repository invokes an outside program to
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
ways and deliberately broken in fourteen. Nothing of it is committed. It is
fetched — `http://www.schaik.com/pngsuite/PngSuite-2017jul19.zip`, with
`TINKER_PNGSUITE` pointing at the extracted directory — and
`crates/tinker-pdf-filters/tests/png_suite.rs` prints `pngsuite-oracle: RAN` or
`pngsuite-oracle: SKIPPED` per test, because a skipped check exits 0 and reads
exactly like a pass.

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
chunk's type and data, and the IDAT payload equal to `zlib_compress` of the
filtered stream.

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
every job that depends on something being present prints a `RAN` / `SKIPPED`
line, or a count of what it measured, and greps its own log. The jobs that
fetch what cannot be committed need it most: what they read can fail to arrive
as well as fail to read, and both look like a green tick. In the corpus job the
grep proves the strict pass ran, because a child that skipped it would leave
every file "not eligible", which reads as a run that measured nothing rather
than as a regression. The native `test` matrix greps its own log too: a leg
that runs the suite inside `cargo test --workspace` says nothing about whether
`determinism.rs` was one of the tests it ran. The same rule holds for CI's own
tooling: every use of `taiki-e/install-action` names its tool explicitly, a
spelling that cannot lose its argument.

## The release

`release.yml` runs on a `v*` tag and on a `workflow_dispatch`, and only a
dispatch carrying `publish: true` reaches its `publish` job, so a tag push
builds and stages every package and publishes nothing.
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
  (data-file provenance gate), `versions`, `check`, `corpus-fetch` /
  `corpus-run` / `corpus-licences`, `release`, `nuget-stage`.

## Clocks: outside the suite, and not nowhere

Clocks are banned from **tests**. `bounds_ledger.rs` bans `Instant::now` from
every source its rows name, so that every hardening bound is a property of an
input rather than of a machine.

They live in `crates/tinker-pdf/benches/` instead — one file a group over a
shared `support/` module, under criterion, which is exempt tooling by name and
whose plotting stack is switched off. `cargo bench` runs weekly in
`bench.yml`, one job a group, and keeps its numbers as artefacts; it is
scheduled rather than gating, because a hosted runner swings widely between
two runs of identical code and a benchmark that fails a pull request on that
teaches people to ignore it. A scheduled workflow runs the default branch,
`main`, and a `workflow_dispatch` is how `develop` is measured. The useful
mode is criterion's own comparison, which needs two revisions on one machine:

```sh
cargo bench -p tinker-pdf -- --save-baseline before
cargo bench -p tinker-pdf -- --baseline before
```

and `--bench <group>` narrows either to one group's file.

Twenty-three operations time `tpdf` itself: tier P marks them CLI, their
figures include the process, and `tpdf` has no library half.
`benches/support/cli.rs` runs this revision's binary from beside the bench
binary's profile directory, one child per call, its output kept in the
operation's own directory and its exit status checked every time, and the
operation's assertions read what that child wrote. It is the one file under
`benches/` that `cargo xtask oracles` lets start a program: the child is this
repository's own, so it hosts and adjudicates nothing. `bench.yml` builds the
release `tpdf` before it measures, and `cargo test --benches` needs a debug
one built first.

**The comparison is `cargo xtask bench-check --machine NAME`, and it exists
because criterion's own cannot fail.** `cargo bench -- --baseline` compares
perfectly well and **never exits non-zero when it loses**: the only failure path
in 0.5.1 is a *missing* baseline directory, and a regression prints
"Performance has regressed" and exits 0. `bench-check` reads criterion's own
`estimates.json` — its numbers, not a second timing — and holds each operation
to a committed figure and a band in `crates/tinker-pdf/benches/baseline.json`.
Each group's job also counts the criterion lines in its log against the count
its matrix entry gives, so a group that measured fewer operations than it
declares fails.

A band has to be above the machine's own swing or it fails on the machine
rather than on the code, so `bench-check` refuses an entry that carries no
measured swing.

### The named machine is `ubuntu-latest`

`bench.yml` runs on `ubuntu-latest` and holds every run with
`bench-check --machine ubuntu-latest`. Its figures are criterion's own
`mean.point_estimate`, read out of the `estimates.json` each run uploads as an
artefact — nothing re-times anything, and the runner hosts this code rather
than judging it (ruling 13). `baseline.json` lists the dispatches its figures
were taken from.

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
split rather than noise, so `bench.yml` writes `/proc/cpuinfo`'s model name and
the CPU count into `bench.log`, so that a measurement can attribute the split
rather than band it.

**What this does not catch.** The widest band passes a change that makes its
operation half again slower in silence; a several-fold regression would not.
This is a ratchet against the catastrophic and not against the incremental,
and the finer instrument is criterion's own `--baseline` between two revisions
on one machine, which needs no band because it changes nothing but the code.

**The entry is checked, not just written.** `cargo xtask baseline` — part of
`cargo run -p xtask -- check`, so it fails a build — holds the files that carry
each group's list of operations to each other: a `[[bench]]` declares each
group file, the file defines the group's operations, `bench.yml`'s matrix gives
the count its job's guard holds the log to, and `baseline.json` lists them, in
the file's order and each with its unit, and records a figure for each or names
it unmeasured. It refuses a name criterion would record under another key — one
holding any of the ten characters it writes as `_`, or longer than the 64 bytes
it keeps — since a name it rewrites is reported as new and never held to its
band. Every band in the committed file is pushed against from both sides by a
unit test, because a band nobody has ever crossed is a number nobody has
checked.

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

The parent holds each case to its build's figure in `peak_memory.tsv`, the
default and `bundled-fonts` builds each with their own, plus the 2 % the corpus
bars give their peaks, and fails a case the table has no figure for. The table
holds none, so a run fails every case unless `TINKER_PEAK_RECORD` is set, which
prints each case's line for the table instead of holding it:

```sh
TINKER_PEAK_RECORD=1 cargo test --release -p tinker-pdf --test peak_memory -- --ignored --nocapture
```

It is Linux-only, since the peak is read from `/proc`, and `#[ignore]`d, since
a render at 300 dpi and ten thousand pages of text are a release binary's
work. What it decides with the numbers — the status line read in KiB, the
band, the refusal, the line the record switch prints — is plain functions its
own unit tests hold on every target, and
`a_child_holding_64_mib_reports_at_least_that` shows that a child's figure
sees what the child holds. The child is this test binary, so the file is a
`SPAWNERS` row: it hosts one case a process and adjudicates nothing.

## The examples, run rather than compiled

`crates/tinker-pdf/examples/` holds the documented end-to-end usage: open,
render, extract, edit, create, convert, parallel. Each runs with no arguments
against a committed fixture, so trying one needs no corpus and no download.

CI **runs each of them and greps its output**, which is the same rule as
everywhere else on this page: `cargo clippy --all-targets` already compiles
them, and a program that compiles and prints nothing is indistinguishable from
one that works. The greps assert content — `the original, untouched` from the
incremental save, `read back page 1 says` from the writer reading back what it
wrote — so an example reduced to an error message fails the job.
