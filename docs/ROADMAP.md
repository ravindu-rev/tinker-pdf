# Roadmap

The goal, stated as a capability target: **every document a real producer
emits opens, renders provably the way the world renders it, round-trips,
signs, conforms — on every target, deterministically, with no unsafe code
and no silent failure.** The feature docs record what already holds; this
file lists what does not yet, and nothing else. What has left it is in git
history and in the design docs' own "as built" sections, not here.

Every item carries its **evidence** (a measured number or a named refusal),
its **exit criterion** (a test or CI job that runs — a roadmap item without
one is not done when the code lands, it is done when the check goes green),
and a **size band**: S ≈ 0.5 engine-months, M ≈ 1–2, L ≈ 2–4, XL ≈ 5–8.
L and XL items have a design doc in [design/](design/); where one is named
as owed, it is written before the item is scheduled. Scheduling within a
tier follows corpus hit-rate evidence (ruling 3, [rulings.md](rulings.md)),
not interest. Every number below was derived from the tree on 4 September
2026 by the command the commit message names; a number this file carries
and the tree contradicts is a defect in this file.

The measurements the tiers are ordered by, from `corpus/ratchet.json` and
its two font-bearing siblings:

| Measure | Value |
| --- | ---: |
| Corpus files (pdf.js 974, veraPDF 2 907, qpdf 637, PDF Association 7, SafeDocs 1 000) | 5 525 |
| Render every page | 5 516 |
| Do not (6 qpdf, 3 SafeDocs; every one a file that would not open) | 9 |
| Rendered with something reported, no faces / synthetic face / bundled faces | 1 412 / 459 / 495 |
| `rotate` held of asked | 4 998 of 5 114 |
| `rotate-tight` held of asked | 4 583 of 5 114 |
| `crop` held of asked | 4 929 of 5 075 |
| `dpi` held of asked | 5 342 of 5 457 |

The suite stands at 6 084 passed, 0 failed, 60 ignored across 266 suites as
[verification.md](verification.md) records it, measured 3 October 2026 on
`x86_64-unknown-linux-gnu`.

## What "best" means here

The target is not a list of features but a set of axes, each with a number
that can only move one way. A capability the field offers and this engine
lacks is a row in tier 5. An axis nobody measures is a row in tier 0, and it
comes first, because a claim with no ratchet behind it is the kind this
repository has already caught itself making. Nothing below names another
engine: a capability is described on its own terms, and "the field" means
what a reader of PDFs is entitled to expect.

| Axis | Measured today | What gives it a ratchet |
| --- | --- | --- |
| Correctness on documents nobody here wrote | 5 525 files: three readers' test suites, one association's examples, and **1 000 documents a crawler found on the open web** spanning 462 distinct `/Producer` strings | **ratcheted**; `corpus/corpora.lock`'s fifth entry, run nightly, every failure attributed by producer |
| Speed | seven criterion operations, weekly; `crates/tinker-pdf/benches/baseline.json` records each one's fastest figure on `ubuntu-latest` over **fifteen dispatches of one revision**, and each one's own band above its own measured swing — 22.61 % for the text extractor, 88.36 % for the text renderer | **ratcheted**; `cargo xtask bench-check --machine ubuntu-latest` in `bench.yml`, which fails where criterion's own comparison exits 0 |
| Memory | 54 caps in `bounds_ledger.rs`, two of them the runtime bounds a *process* spends and one of them a relation between a symbol and its page rather than a quantity; every corpus child measures its own peak resident set, `report.json` carries it per file and `ratchet.json` bands the per-corpus maximum within a **measured 2 % tolerance** — a high-water mark swings 0.06 % to 0.76 % between two runs of one binary, and an exact band failed on that within a day of being recorded | ratcheted, over five corpora |
| Fidelity | arithmetic fixtures, metamorphic relations, committed fingerprints | tier 1's differential pairs and reviewed goldens; ruling 13's amendment of 5 September 2026 on dated outside measurements |
| Capability coverage | tiers 2 to 5 of this file | each row's exit criterion |
| Footprint | 2.03 MB of wasm, 1.40 MB gzipped, gated at 2.5 MB in `release.yml` | already ratcheted |
| Surface | 123 C functions; four bindings, none projecting the whole facade; eighteen CLI subcommands, ten reading and eight writing, every writer taking the font policy | tier 3's bindings row; tier 5's CLI and bindings rows |
| Maturity | version 0.0.1, nothing published, one release run watched | tier 3's packages row |

## Tier 0 — measure what is not measured

These came before any new feature, with tier 1. Each row was an axis the
field judges an engine on and this repository had no number for, so a claim
about it would have been the kind of claim ruling 13 exists to prevent.
**All four are closed.** What closed them is kept here rather than deleted,
because these measurements are the reason the rest of this file may quote a
number at all.

**Two of the four rows closed on 5-6 September 2026 and one of them paid for
itself immediately.** The production-corpus row is closed: `corpus/corpora.lock`
pins the DARPA SafeDocs shard, a thousand documents a crawler found on the open
web, spanning 462 distinct `/Producer` strings — and its first run said fourteen
of them could not be rendered inside a minute. That was not the corpus and not
the timeout: it was two loops in the rasteriser indexing with a bounds check per
pixel, which cost **9× on a page of a real Word document** and which no fixture
corpus had ever said a word about. Fixing it closed the vectorisation row in the
same measurement, with every fingerprint unchanged on every target
([verification.md](verification.md), "The rasteriser's row loop").

What the production corpus refuses is three files of a thousand, and not one is
a page this engine drew wrongly: one is genuinely encrypted, and two are HTML
pages the crawler saved under a `.pdf` name.

**And it settled the measurement three ledger rows were waiting on.**
`MAX_JBIG2_SYMBOLS`, `MAX_JBIG2_SYMBOL_PIXELS` and `MAX_JBIG2_TEXT_INSTANCES`
published the word "estimate" because the largest `SDNUMEXSYMS` anywhere in the
corpus was **11**, every one of them synthetic, so a cap calibrated on them
would have admitted anything. Over 118 JBIG2-bearing files including the
production corpus's, the largest is **2 478 exported symbols, 2 468 new ones
and 4 440 text instances** — real OCR output, from documents scanned by people.
The caps stand at 100 000 symbols and 4 194 304 instances, which is 40 and 944
times what a real document has asked for.

**Two of those three settled; the middle one settled on 26 September 2026 and
brought a fourth row with it.** `MAX_JBIG2_SYMBOL_PIXELS` is a *pixel* budget
and the figures above are counts, so none of them was its yardstick — and the
reason nobody had taken its measurement is a fact about the format rather than
an oversight: **neither of a symbol's dimensions is in any segment header**, so
`jbig2_census.rs`, which shares no code with the decoder on purpose, could count
symbols and could not measure one. It decodes for that half now. The largest
dictionary in five corpora spends **1 568 118** pixels, which the cap clears by
42.8x. The same pass answered the question `verification.md`'s open `jbig2` fuzz
row had been left on — the largest symbol *relative to its page*, which over
88 736 corpus symbols is **never more than one** — and
`MAX_JBIG2_SYMBOL_PAGE_MULTIPLE` is the bound chosen from it, at 4. That row is
closed.

**And the fourth closed on 19 September 2026: speed has a ratchet.** The row
said it was *"blocked on runs of a machine nobody owns"*, and it was not.
`bench.yml` has a `workflow_dispatch` and runs on `ubuntu-latest`, so the
machine was one command away the whole time. Fifteen dispatches of one
unchanged revision measured that runner's own swing over the seven
operations — 22.61 % for "extract a page of text", 88.36 % for "render text
at 150 dpi" — and `crates/tinker-pdf/benches/baseline.json` now carries each
operation's fastest figure of the fifteen and its own band above its own
swing. `cargo xtask baseline` holds that file, `benches/engine.rs` and the
weekly job's count to each other, and all fifteen runs pass the entry they
set ([verification.md](verification.md), "Clocks: outside the suite, and not
nowhere").

**The row's diagnosis of why the weekly job had never worked was half wrong,
and the half that was wrong is the one worth keeping.** The guard *was* the
first fault and it *was* fixed, on 5 September. The job went on failing every
week afterwards for a different reason entirely: **a scheduled workflow runs
the default branch**, which is `main`, hundreds of commits behind `develop`
and still carrying the `time:+\[` that cannot match. So the cron runs of 31
August, 7 and 14 September each benchmarked a months-old tree — six
operations, not seven — and the fix on `develop` could not reach them.
Dispatched on `develop` instead, the job reported seven operations fifteen
times out of fifteen. Whether `main` should be moved to `develop` is a
release decision and is not taken here; until it is, the weekly run is a
measurement of `main`.

**What the ratchet does not do, said before anybody relies on it.**
`ubuntu-latest` is a fleet and not a machine: scored by the geometric mean of
their seven operations, the fifteen runs fall into two groups with nothing
between them — four at 1.00 to 1.16 of the fastest and eleven at 1.30 to
1.36 — on one and the same runner image, and nothing in the log said why. So
the job records `/proc/cpuinfo`'s model name now, and the next measurement can
attribute the split instead of banding it as noise. A band above that spread
is wide. The
text renderer's is 135 %, so a change that makes it half again slower passes
in silence; the 9× rasteriser regression of 5 September would not have. And
the swing has not converged — it read 57.59 % over the first five runs,
72.71 % over nine and 88.36 % over fifteen — so these bands are a floor that
a later measurement may have to raise. The finer comparison is still
criterion's own `--baseline` between two revisions on one machine, which this
does not replace and does not claim to.

## Tier 1 — prove correctness

These come before any new feature, and with tier 0 closed they come first.
Ruling 13 says the engine agreeing with
itself is the only kind of proof this repository will have, which raises the
bar on what the checks must be: answers computable in closed form, bitstreams
transcribed from the standards' own annexes, published conformance data, and
thousands of documents nobody here authored. What this tier cannot contain is
stated once: the four properties that left with the oracles
([verification.md](verification.md), "What does not come back") do not return
and are not rows. Neither is JPEG XR's unadjudicated list, for the same kind
of reason — it is a licence limit, and it is under Named non-goals below.

**Two rows closed on 5-6 September 2026, both by taking a decision that had
been recorded and deferred.**

The **thirty-six password-refused files** are measurements now:
[corpus/passwords.tsv](../corpus/passwords.tsv) carries 34 rows, each quoted to
the upstream line that states it, two marked `derived` because no upstream file
names them, and one carrying a SASLprep-normalised form with the gap that
requires it named rather than hidden. Two files stay refused and neither is a
password: one misspells its key length as `/Wength 128`, and one is 91 bytes of
qpdf's own fuzzer output with `/O` and `/U` both empty.

**`ROTATE_BUDGET` is 10 %**, and the measurement that raised it also corrected
the measurement that prompted the row. The 2.64 % and 2.81 % this row used to
quote were properties of the *fixtures* — 64-point pages with the construct in a
corner. On pages the construct covers, a page of 11-point text costs **8.77 %**
and stays there as the page grows, so two percent was out by a factor of four
and every text-heavy document was failing the relation for arithmetic. Ten sits
above that and below the tiling class at 22 %. It costs signal — 181 corpus
failures become 15 — so the same measurement is judged again at 3 % and recorded
as its own relation, `rotate-tight`, which the ratchet holds and no run fails
on. One limitation is asserted rather than left to be rediscovered: a page
saturated with diagonal edge costs 23.5 %, which is the defect class's own
range, so no budget separates those two populations.

**The Annex F numbering row closed on 20 September 2026, and reading the
clause corrected the row three times.** The two groups are the other way round
now — parts 7, 8 and 9 are numbered sequentially from 1, so Table F.4 item 1's
"the first object of the second page shall have an object number of 1" is true
by construction, and the head is numbered after them. The row's own text was
wrong about what the head holds: it is not only the catalogue, the
document-level objects and page one but **part 2 and the primary hint stream
as well**, because the first-page cross-reference section is one subsection and
7.5.4 makes a subsection a contiguous range. There was no conflict with
"reserved" objects 1, 2 and 3 to resolve, either: Annex F reserves no object
numbers, and the reservation was this writer's own device to keep a front
section headed `0 N` from freeing everything the main table carried. The front
section now starts at the head group's own first number and frees nothing,
which makes the 60-page streaming fixture **twenty bytes shorter** — one
cross-reference entry — and moves every offset in it. Third, F.3.6 gives the
hint stream
*the last object number in the file* whatever its physical position, which
qpdf's `lin1.pdf` shows and nothing here had read — so it is numbered after
page one's run and still written before it.

The exit criterion is
`crates/tinker-pdf-cos/tests/linearized_numbering.rs` and
`validate/hints.rs`'s `annex_f_numbering`: Annex F's arithmetic — `/O`, then
1, then each page's declared object count added to the one before — finds
every page of this writer's output at one, two, three and six pages, plain and
encrypted, and of **31 linearized files in the fetched qpdf corpus**. One does
not, and it is named with the reason: `badlin1.pdf`, qpdf's deliberately
damaged fixture, whose `/O` is one object late. The three streaming budgets
were re-measured and none moved.

**One thing the flip found that was not in the row.** The strict validator's
`/E` rule recomputed page one's reach from a *run of consecutive object
numbers*, and `page_run(0)` returns nothing when no page is numbered above page
one — which is every conforming file, so the rule had been **inert on every
corpus file it had ever seen** and only fired on this writer's own
non-conforming numbering. It recomputes by reachability now, which is what `/E`
promises a streaming reader and is independent of the numbering. F.3.7's own
exclusions come with it — page tree nodes, other page objects, and `/Thumb` —
plus `/EF`, which F.3.7 does not name and Table F.2's `B` hint table implies;
each of the three was added because a file in the corpus reported without it,
and each is named with that file in `validate.rs`. Measured 20 September 2026
over the 45 linearized files in the fetched qpdf corpus: **none reports `/E`**.

| Item | Evidence | Exit criterion | Size |
| --- | --- | --- | --- |
| **A stroke under a transform that is not a similarity is drawn at one width.** 8.4.3.2 measures the line width in user space, so an anisotropic scale or a shear widens a stroke more in one direction than another; `stroke_path` strokes in device space at the user-space width times `Matrix::expansion` (√\|det\|) and scales the dashes by the same number, so under `scale(1, 3)` a circle stroked two wide is drawn 2√3 ≈ 3.46 device units all round by that arithmetic where the clause makes it six at top and bottom and two at the sides. The SVG writer states the same pen. Found by the appearance-synthesis lane, whose squiggly underline is drawn to avoid it | `crates/tinker-pdf-render/src/lib.rs`'s `stroke_path` (`let scale = state.ctm.then(&self.base).expansion()`, and the dashes `d * scale`); `Matrix::expansion` in `tinker-pdf-content`'s `state.rs`; [features/rendering.md](features/rendering.md)'s "A known defect" | a circle stroked at width 2 under `scale(1, 3)` covers six device units at its top and two at its side, and a dash on a sheared line is sheared, in the renderer and in the SVG writer, each pinned by a test that fails today | M |
| The nine reviewed goldens have not been reviewed. The mechanism is done — `render_goldens.rs` parses each `.ppm`'s header, refuses a field that is absent, blank or whitespace, re-renders every family and compares byte for byte, and holds a size ceiling so reviewing one stays a real act — and `UNREVIEWED` lists all nine families because no person has read them | `UNREVIEWED` in `crates/tinker-pdf/tests/render_goldens.rs`, counted; the three injections on the mechanism are each caught by exactly the check written for them | a person reads each golden against the clause its header names, their name and the date replace `unreviewed` in that header, and `UNREVIEWED` empties | S, and it is a reading rather than work |

## Tier 2 — close the named refusals, by measured reachability

**What is left of this tier, stated plainly.** The JPEG arithmetic row
**closed on 20 September 2026** — SOF9 and SOF10 decode — and what closed it
was the third wall coming down: T.81 Annex K.4.1 publishes a complete
arithmetic-coder test vector, which nothing here had looked for. **The JPX row
closed on 23 September 2026**, when `BYPASS` and `TERMALL` landed and Table
A.19 left the refusal list entirely — the last of the capabilities that row
named, after RGN, PPM, PPT and POC on the 20th and 21st. **One row is left, and it is
not scheduled work**: the arithmetic JPEG *statistical model*, which is a named
limit — it is transcribed, it is pinned as well as a reading can be, and
nothing published will adjudicate it. Three walls, each checkable, and
re-checking them is what has moved every one of them that has moved:

- **Zero corpus reachability**, measured and pinned by a census that runs
  nightly — `jpeg_census.rs` walks 10 606 JPEG streams and finds no arithmetic
  frame, `jpx_attribution.rs` walks 39 JPX files and finds no coding
  capability among the refusals. Closing PPM and PPT left it unchanged, which
  is the only result consistent with the claim.
- **The specification is not obtainable — and for T.800 this was wrong.**
  T.88 is published free of charge and was fetched in September 2026, which is
  what closed the Annex B row above. **T.800 is published free of charge too,
  and was fetched on 13 September 2026**: 231 pages, born digital rather than
  scanned, and its text extracts cleanly with this repository's own `tpdf
  text` — RGN, POC, PPM, PPT and CRG are all there, and so is B.10.7.2's rule
  for how many codeword segments a packet signals, which this file used to say
  would have to be written from memory. The claim that ISO and the ITU both
  sell it was never retested after it was first written, which is the third
  time a "not obtainable" assertion here has turned out to be a WAF page or an
  untried URL.

  **And the fourth time was T.81, on 14 September 2026.** The ITU's own
  publication endpoint does redirect to its shop, and the freely published W3C
  copy this file already named returns 403 to `curl` — which is bot protection
  rather than absence. Fetched another way it is a 1 MB PDF of 186 pages, and
  **Table D.3's 113 states are legible**: `0  X'5A1D'  1  1  1`, with the
  columns the text layer could not separate.

  **The method, because it generalises and nothing here had written it down.**
  A specification whose *text layer* is garbled is not unreadable. Its fonts are
  usually not embedded — this engine bundles none, by policy — so a render draws
  the punctuation and drops every letter, and Word emits equation glyphs
  right-to-left so extraction interleaves them. Render the page with a face
  supplied (`tpdf render --fonts <a Times face>`) and read the image. Proven on
  three documents in one day: T.800's equations (H-1) and (H-2), T.82's DECODE
  flow diagram, and T.81's Table D.3 — all three of which this file had recorded
  as untranscribable figures or digits without field boundaries.
- **No producer exists here** for a fixture, so the third route — build it and
  hold it to something — is closed too. **For JPX this was wrong as well**:
  T.800 Annex J.10 publishes a complete 100-byte codestream, annotated field by
  field, with its nine decoded samples stated in J.10.5. It needs no producer,
  and `crates/tinker-pdf-filters/tests/jpx_annex_j.rs` now decodes it and
  asserts them — the first check in this decoder that is neither a round trip
  through code written here nor a recording of what another program did once.

  **And it stretches further than one codestream, which was the surprise of
  20 September 2026.** J.10.3 and J.10.4 do not only publish the bytes; they
  publish *where each packet's header ends* — Table J.20 lists the first
  header's three bytes, J.10.4 gives its body's offset as octal 0125, Table
  J.21 lists the second header's four bytes and J.10.4 gives its body's
  offset as octal 0137. A published header/body boundary is exactly what a
  packed packet header needs, because A.7.4 defines the packed contents as
  "exactly the packet header which would have been distributed in the bit
  stream": the relocation is then a rearrangement of published bytes across a
  published boundary, judged by J.10.5's published samples, and PPM and PPT
  needed no hand-authored codestream at all.

  *This paragraph used to end with a sentence that was wrong, and it was
  wrong in the direction that stops anyone looking.* It read: "Whether J.10
  can be re-expressed for RGN, POC, `BYPASS` or `TERMALL` is a different
  question with, so far, a different answer — none of those four can be
  reached by rearranging a codestream that does not use them." **POC can be**,
  and it was, on 21 September 2026. A published header/body boundary is not
  only what a *packed* header needs; it is what a *reordering* needs, because
  it says where one packet ends and the next begins. J.10.1's COD declares one
  decomposition level, so B.12 gives that codestream two resolution levels and
  exactly two packets — nine bytes then seven — and swapping them under a
  two-volume POC is a rearrangement of published bytes across a published
  boundary, judged by J.10.5's published samples, exactly as the PPM and PPT
  relocation was. `jpx_poc.rs` is that test. The sentence was written from the
  axes that *look* like progression — layer, component, precinct, of which
  J.10 has one each — and never checked against the fourth, which is the one
  it has two of.

  *What survived of it was narrower, and on 23 September 2026 that was wrong
  too.* It read: a capability whose effect is confined to *inside* a packet
  cannot be reached this way, because rearranging packets does not change what
  is in one — and `BYPASS` and `TERMALL` are both of that kind, so both still
  expect to need a hand-authored codestream. **Neither needed one for its own
  semantics.** The one codestream this lane did author by hand carries two
  layers, and it is there for B.10.7.2's *bookkeeping* — that a layer boundary
  is not a codeword segment boundary — rather than for either style bit; the
  injection campaign asked for it, by firing nothing without it. The old
  premise is sound and the conclusion does not follow, because these two
  capabilities are not confined to inside a packet: B.10.7.2 makes the
  *number of lengths a packet header signals* a function of Tables D.8 and
  D.9, so a style bit that changes nothing about where a packet sits changes
  how its header parses. J.10 publishes two packet headers field by field
  (Tables J.20 and J.21), and that is enough to bracket D.6's boundary from
  both sides without moving a byte: its second code-block's seven coding
  passes are all before the boundary, so with `BYPASS` set the header must
  still signal Table J.21's single three-byte length and those three published
  bytes must still decode to J.10.4's "1, 5, 1, 0"; its first code-block's
  sixteen passes straddle it, so with `BYPASS` set B.10.7.2 wants five lengths
  where J.10 prints one and the published header stops being readable at all.
  **The lever was lever 2 the whole time** — it just reads the header rather
  than locating it. `TERMALL` is the one that really is out of reach, and the
  closure paragraph after the levers says so in its own words.

  The same transcription found an **erratum** in J.10.3: its closing sentence
  puts the second packet header at octal 0134, where Table J.21 immediately
  below it lists `0xC0`, the byte at 0133, and J.10.4's 0137 for the second
  body agrees with 0133. Three statements against one; the three are used.

  **And it was wrong for JPEG arithmetic too, which is what closed that row.**
  T.81 **Annex K.4.1** publishes a 256-bit test sequence, the 32 bytes it
  encodes to, and Tables K.7 and K.8 — a symbol-by-symbol trace of the encoder
  and of the decoder, 256 events each, with `Qe`, `A`, `C` and `CT` per event.
  `crates/tinker-pdf-filters/src/qm.rs` decodes the published bytes to the
  published decisions and encodes the published decisions to the published
  bytes. **That is three documents in a row whose annexes published a vector
  this file had assumed did not exist**, and the assumption was never tested
  before being written down. The rule that generalises: *read the annex list
  before asserting that a standard publishes no data.*

  **The fifth search came back empty, and it is the useful one.** On 20
  September 2026 every annex of T.800 was searched for published ROI data
  before the RGN capability was written, and there is none: Annex H is prose
  and seven equations, K.4 is a bibliography, and `0xFF5E` appears only in
  Tables A.2 and A.24. So the rule is not "the data is always there" — it is
  that the search is cheap and the assumption is not. What the search *did*
  turn up is a second lever worth naming: **J.10 publishes its intermediate
  coefficients as well as its samples**, so a capability that acts on
  coefficients can be adjudicated against the standard's own numbers even
  when the standard prints no codestream for it. `jpx_annex_h.rs` is that
  pattern.

  **The sixth came back empty too, and turned up the third lever.** On 21
  September 2026 every annex was searched for published POC data before the
  capability was written: `0xFF5F` occurs exactly twice in the 231 pages, in
  Table A.2 and Table A.32, and the string "POC" does not occur anywhere
  outside Annexes A and B — not in Annex J's fifteen subclauses, not in K's
  bibliography. The only POC *field values* the standard prints are Table
  A.45's Profile-0 constraint ("If the POC marker is present, the POC marker
  shall have RSPOC0 = 0 and CSPOC0 = 0") and Table A.46's parameter sets for
  the digital-cinema profiles, and both are values for a profile with no
  image, no bytes and no decoded result.

  **The seventh came back full, and it is the one that closed the row.** On
  23 September 2026 every annex was searched for published code-block style
  data before `BYPASS` and `TERMALL` were written, and the clause numbers are
  worth writing down either way. *Normative, and all in Annexes A, B and D:*
  Table A.19 defines the six style bits and reserves bits 6 and 7; Table A.45
  spells the same byte a second way as Profile-0's mnemonic `00sp vtra` with
  `a = r = v = 0`; D.4 and Table D.8 give the two termination patterns;
  D.6 and Table D.9 give the bypass schedule bit-plane by bit-plane, D.4.1 the
  0xFF extension, (D-2) the raw sign and D.6's NOTE 2 the raw stream's own
  0xFF extension; B.10.7.1 and B.10.7.2 give the length signalling. *And two
  of those clauses publish worked examples*, which is what no earlier search
  found in Annex B: B.10.7.1's NOTE 1 prints four layers' lengths, pass counts
  and a valid bit sequence, and **B.10.7.2's NOTE prints a bypassed
  code-block's five included passes, the set `T` they produce, the four
  lengths signalled, their pass counts and a valid 39-bit sequence coding
  them**. That is the standard's bytes in and the standard's numbers out for
  the mechanism both capabilities share.

  So there are now **four** levers — the fourth found on that search, in
  Annex B rather than Annex J — and a capability should be asked which one it
  fits before anything is hand-authored:

  1. *It acts on coefficients* — J.10.4's published intermediate values
     adjudicate it (`jpx_annex_h.rs`, RGN).
  2. *It acts on where a packet's header or body sits* — J.10.3's and
     J.10.4's published packet extents adjudicate it (`jpx_annex_j.rs` for
     PPM and PPT, `jpx_poc.rs` for POC).
  3. *It acts inside a packet's codeword segments* — B.10.7.2 makes this
     visible in the packet *header*, because `K`, the number of lengths
     signalled, is a function of Tables D.8 and D.9. So lever 2 reaches it
     whenever the standard publishes a header with a pass count, which J.10
     does twice (`code_block_styles.rs`, `BYPASS`). It reaches `TERMALL` only
     if `K` can come out as it was published, and for J.10's sixteen- and
     seven-pass code-blocks `TERMALL` makes `K` sixteen and seven against a
     printed one and one — so that half is transcribed and not adjudicated.
  4. *It is a rule about the packet header's own bits* — B.10.7.1's NOTE 1 and
     B.10.7.2's NOTE each print a complete valid bit sequence with the lengths
     and pass counts it codes, and those are the standard's bytes in and the
     standard's numbers out. This is the fourth lever and the one the seventh
     annex search turned up; it is the strongest evidence either of these two
     capabilities has.

Any one of the three moving is what schedules the remaining rows. **All three
moved for JPEG arithmetic**, so that row is closed. **Two moved for JPX** — the
specification is in hand and Annex J.10 adjudicates a decode — which scheduled
its row, and the row then closed on 23 September 2026 with `BYPASS` and
`TERMALL`. Zero corpus reachability never moved for either, and under ruling 3
that was a scheduling input rather than a wall throughout; the census says the
same thing after this closure as before the first.

**What the JPX row left behind, since the row itself is gone.** Its last six
capabilities left in four days — RGN, PPM, PPT and POC on 20 and 21
September 2026, `BYPASS` and `TERMALL` on the 23rd — and Table A.19 is the one
entry that went to **zero** rather than narrowing: all six code-block styles
decode, and what fires for A.19 now is a style *bit* the table does not
define, which is bits 6 and 7 and a value rather than a capability. The two
were **two capabilities over one mechanism**, which is what the old note in
`cb_style` had half right: it called the pair one change and "not a tier-1
change". `TERMALL` really is only about where a codeword segment ends, so that
half held; D.6 makes `BYPASS` read some passes as raw bits, which is squarely
tier-1, so the other half did not. What they share is B.10.7.2's multiple codeword segments, and that is
the part T.800 adjudicates — B.10.7.2's own NOTE prints a bit sequence, the
set `T` it implies under D.6, and the four lengths and pass counts it codes,
all of which `code_block_styles.rs` requires back out of the shipped reader
with `T` derived from this build's Table D.9 transcription rather than written
in. **`TERMALL` has no link whose expected output is T.800's** and the roadmap
should not pretend otherwise: Table D.8 publishes the pattern and no bytes,
and J.10's two code-blocks carry sixteen and seven coding passes against
headers that print one length each, so `TERMALL` cannot be flipped onto them.
What holds it up is Table D.8 transcribed and asserted as data, plus the fact
that it shares `read_lengths` with the half B.10.7.2 does adjudicate. The
census after the closure is the fifth unchanged run: **same 39 bearing files,
same 7 refusals, same five reasons**.

**What the closed row's adjudication does and does not cover, because the two
halves are not the same.** K.4.1 adjudicates Annex D's *coder* outright. It
says nothing about the *statistical model* of F.1.4.4 and Table G.2 that picks
the coder's contexts, and **T.81 publishes no arithmetic-coded image** to
adjudicate that with — searched annex by annex on 20 September 2026, with the
search written into `jpeg.rs`'s header so nobody repeats it. So the model is
transcribed from its clauses, read twice, and pinned against hand-derived
decision sequences that assert the statistics bin of every decision as well as
the coefficients. That is weaker than K.4.1 and the row says so.

The Annex B work remains the argument for not proceeding without a
specification, and a second measurement now says the same thing from the other
side. Six implementation specs for the JPX capabilities were drafted from
T.800's text and then adversarially checked against it clause by clause:
**37 of 182 citations were wrong** — a fifth of them — including clause numbers
that do not exist, quotes that were paraphrased, and one table's contents
attributed to another table entirely. Only one of the six came back clean. So
having the specification is necessary and is not sufficient: what makes a
transcription safe is checking it against the document a second time, which is
what caught four wrong Annex B tables and what caught these.

**What the JBIG2 rows left behind, since it is the number this tier was
measured by.** **1 of the 118** JBIG2-bearing corpus files still reports a
refusal — unchanged by `MAX_JBIG2_SYMBOL_PAGE_MULTIPLE` on 26 September 2026,
which was the condition on that bound landing at all — down from 34, and it is `pdfjs/issue3371.pdf` — a stream that stops
inside a segment, which `Jbig2Refusal::is_malformed` calls damage rather than a
capability. Halftone, custom code tables, transposed placement, 7.2.7's unknown
data length, a blank page, 7.4.2's retained bitmap-coding contexts, a text
region that places nothing, 6.5.8.2.2's reference offset and four
mis-transcribed Annex B tables have all left. `jbig2_attribution.rs` pins the
count at one and names the file. **Every real-world document in five corpora
decodes.**

| Item | Evidence | Exit criterion | Size |
| --- | --- | --- | --- |
| ~~**JPEG arithmetic coding**~~ **SOF9 and SOF10 decode, landed 20 September 2026. SOF11, SOF13, SOF14 and SOF15 are refused by the annex they need rather than by the coder they use, and that re-labelling is the row's other half** | `crates/tinker-pdf/tests/jpeg_census.rs` walks every `/DCTDecode` stream through the COS layer, so encrypted documents and object streams are seen: **688 files, 10 606 distinct streams, 10 603 frames, and every one of them is SOF0, SOF1 or SOF2 at eight bits**. Zero arithmetic, zero lossless, zero other precision — re-measured 20 September 2026 over 5 605 files, and unchanged over the 5 605 files `TINKER_CORPUS` held that day, which is the five corpora above plus what else the cache carries. That is the largest population any census here measures | **closed for SOF9 and SOF10.** The third blocker fell: **T.81 Annex K.4.1 publishes a 256-bit test sequence with the 32 bytes it encodes to**, plus Tables K.7 and K.8's per-event traces — so the coder is adjudicated by the standard's own bytes in both directions, and `qm.rs` holds it as a permanent fixture. Table D.3's 113 rows were read twice, off a rendered page and out of the text layer, and the two readings agree on every `Qe`; the 27 rows the text layer cannot disambiguate are named, because T.81's column rules extract as a literal `1`. `JpegError::Arithmetic` is **gone**: SOF11 and SOF15 report `Lossless` (Annex H's predictor) and SOF13 and SOF14 `Differential` (Annex J), which is what the refusal was always meant to name. A defect found on the way: the DAC marker `X'FFCC'` was **skipped** while a comment claimed it was handled — the same shape as the SOF3/SOF5/SOF6/SOF7 defect this row already recorded | closed; what is left open is named in the row below rather than here |
| **The arithmetic JPEG statistical model is transcribed and not adjudicated, and nothing published will adjudicate it.** F.1.4.4's Tables F.4 and F.5, Table G.2, and B.2.4.3's DAC conditioning are read off the clause; the coder under them is pinned by K.4.1 | **The search, so nobody repeats it** (all 20 September 2026 unless noted). *T.81*: `https://www.w3.org/Graphics/JPEG/itu-t81.pdf` → **HTTP 200**, 1 058 883 bytes, SHA-256 `631031d4…768bf0`; Annex K read section by section — K.1/K.2 quantisation tables, K.3 Huffman tables and K.3.3's byte lists, **K.4.1 the arithmetic coder vector and K.4's only subsection**, K.5 to K.10 filters and guidance with no data; a sweep for hexadecimal runs over the whole document finds only those, plus `X'FFFF0000'` in D.1. *T.83* (ISO/IEC 10918-2, the compliance-test document): `https://www.itu.int/rec/dologin_pub.asp?...T-REC-T.83-199411-I!!PDF-E...` → **HTTP 500** again; the standards-preview extract `https://cdn.standards.iteh.ai/samples/20689/…/ISO-IEC-10918-2-1995.pdf` → **HTTP 200**, 2 642 839 bytes, SHA-256 `6ceaa9ff…b636f358`, and `tpdf info` counts **15 pages** — the front matter and numbered pages 1 to 11, exactly as this file recorded in September, and clause 4.4 is in it: the data "are available on 3 diskettes and are included with the copy of this ITU-T Recommendation" rather than in the document. *T.84* (ISO/IEC 10918-3): `https://www.itu.int/rec/dologin_pub.asp?...T-REC-T.84-199607-I!!PDF-E...` → **HTTP 200**, 419 607 bytes, SHA-256 `cd714dd1…89f6173f`, and `tpdf info` counts 84 pages — read here, and clause 4.2.1 says compliance data is "available from ISO and ITU to parties who wish to determine compliance"; Annex G specifies the *structure* of the test streams and prints none | **stays open as a named limit rather than as work.** What would close it: a real document, and the census walked 5 605 of them on 20 September 2026 and found none; or published data, and three documents have now been searched. What would **not** close it is an arithmetic encoder written here — ruling 13, and it would only prove the two halves of one reading agree. What holds it meanwhile: the decisions of each model are hand-derived from T.81's own figures and the tests assert **which statistics bin every decision was taken against**, not only the coefficients, because a wrong bin decodes correctly until the bins have adapted | not sized; it is a limit, and it moves only if a document or a vector appears |

## Tier 3 — capabilities absent today

Ordered by leverage, not size.

| Item | Evidence | Exit criterion | Size |
| --- | --- | --- | --- |
| **Reading order of a right-to-left line in text extraction.** A searchable Arabic EPUB extracts backwards: `TextLine::rtl` is a report and `plain_text` follows content-stream order. Reversing a line by `rtl` is a decision about every PDF this engine reads, so it is pinned with its reasoning and not fixed | `crates/tinker-pdf/tests/epub_shaped.rs` pins the backwards line by name | a recorded decision, and the pin flipped to assert it | M |
| **The Universal Shaping Engine's remainder.** 32 of 333 Brahmic cases are not reproduced and nine of sixteen sections are not whole; one cause inside USE's cluster grammar is recorded and priced: Kannada `SHKNDA-2/1` and Tai Tham `SHLANA-2/6` are one cluster shape wanting opposite orders, no property this crate reads separates them, and `universal.rs` measures the fix at six gained for sixty-nine lost. This row used to say two such causes "account for 21 of the 32"; that split is not derivable from `TRIAGE`, whose verdicts are 25 Set, 2 Order, 4 Advance and 1 Offset, and `SHLANA-2/6` is not in `TRIAGE` at all because it passes. Named debts beside it: the Indic base-finding model, reph position, canonical ordering by combining class, Hangul decomposition, per-syllable GSUB confinement, `Default_Ignorable_Code_Point` | [design/shaping.md](design/shaping.md) milestone 5, "not met, and not relaxed"; `PASSING` and `TRIAGE` in `crates/tinker-pdf-shape/tests/text_rendering.rs`; [features/fonts.md](features/fonts.md) | sections whole, asserted by count; the priced alternative is a second cluster model | XL ([design/shaping.md](design/shaping.md)) |
| Shaping consumers still refused by name: a `CIDFontType0` with a bare CFF in a form field (`Shaper::new` takes an `&Sfnt`), a vertical CMap in a shaped fill, a simple `/DA` font; on the EPUB page, GPOS offsets below the `TextRun` level, and bidi whose unit is the run rather than the visual line, so a right-to-left line of two styled spans is two runs | [features/forms.md](features/forms.md), [features/fonts.md](features/fonts.md) | each refusal replaced by a fixture that renders joined or positioned | M |
| Scripts shaped but unverified — Syriac's Alaph, the topographical features of a joining Brahmic script, and the list by name in [features/fonts.md](features/fonts.md) | ruling 13 leaves a script with no first-party conformance fixture unadjudicated | a fixture per script, or the name stays on the list | — |
| **The PDF/A validator's 39 staged rules.** Agreement with the veraPDF corpus is 1 948 of 2 371 PDF/A files — 830 of 831 `pass`, 1 118 of 1 540 `fail`. The unagreed `fail` files fall into families: **conformance level A is validated now** — `/MarkInfo`, the structure tree root, every structure type after the `/RoleMap`, and every `/Lang` the object graph carries, which took 19 of the 21 logical-structure fixtures and left `830 of 831` where it was; **the predefined schemas' value types are read now** — the three RDF containers are told apart rather than collapsed to one array, and a property's text is read against the grammar the XMP specification prints for its declared type (`Integer`, `Real`, `Boolean`, `Date`), which took 167 of the 168 files and left `830 of 831` where it was; what stays staged there is the types no fixture exercises (`Rational`, `URI`, `GPSCoordinate` and the rest), since 467 of the 468 `-fail-` fixtures in those two directories are already reported on; then four on an extension schema's custom value types, and six on the packet header's `bytes` and `encoding` attributes — now a staged rule of its own at 6.6.2.1, because the `<?xpacket?>` instruction is consumed before the first element is seen — a /Metadata stream that carries a filter, and a packet Isartor calls malformed; then 197 in graphics clauses that run and not far enough — the content-stream operator list, the Separation tint transform and the destination profile's own conformance, none of which is a prohibition over a dictionary — 97 in font clauses waiting on code-to-glyph mapping, content-stream rules as an interpreter rather than a tokenizer (which is what level A's two remaining logical-structure fixtures wait on, and 6.2.11.7.3's five `/ActualText`-per-character ones with them), recursive validation of embedded files, the output intent's own ICC conformance, and 6.11 and 6.12 with fixtures and no rules | `PDFA_STAGED` (the `lib.rs` re-export of `STAGED` in `crates/tinker-pdf/src/pdfa.rs`), counted; [features/pdfa.md](features/pdfa.md); [design/pdfa.md](design/pdfa.md) | the staged count moves down and the agreement ratchet up, one ledger class at a time | L ([design/pdfa.md](design/pdfa.md)) |
| **Tagged writing.** `/Alt`, `/ActualText`, `/E` and `/Lang` are not written on structure elements; there is no general tagging API beyond `PageBuilder::tagged`; the EPUB's tree carries no `/Alt` on images, no `/Lang`, no `<a>` as a `/Link` with its `/OBJR`, no table `/Headers`, `/Scope` or `/Summary`, and no `/RoleMap` | [features/content-and-text.md](features/content-and-text.md), [features/epub.md](features/epub.md), [design/tagged-pdf.md](design/tagged-pdf.md) | `epub_structure.rs` asserts each against the source XHTML; the PDF/UA census runs over this engine's own output | M |
| **Signatures.** **Every named shape has left; one corpus measurement is owed.** **RSASSA-PSS has left** — verified against 360 CAVP `SigVerPSS` vectors, RSA Laboratories' 60 and an OpenSSL-signed document (`signature_shapes.rs`). **A signature with no signed attributes has left too** — checked over the covered bytes' own digest, against an OpenSSL `-noattr` fixture; the corpus's one such signer (`bug854315.pdf`) is owed its first measured verdict, which `verdicts.rs` prints and tallies apart. **`adbe.pkcs7.sha1` has left** — both links of its encapsulated digest checked, against two OpenSSL fixtures; its one corpus file is a fuzzer's mutation whose coverage does not hold up, so no corpus count moves. **`GeneralNames` have left** — decoded in the alternative-name extensions, `authorityCertIssuer` and an ESS `issuerSerial`, against RFC 5280 C.2 and an OpenSSL CAdES signature. **RFC 3161 signature timestamps have left** — imprint, signature, `TSTInfo` digest, the authority certificate's EKU and ESS binding, and its chain at `genTime`, against a token OpenSSL's TSA made. What is left is measurement, not work, none of it taken because the corpora could not be fetched where these landed: `verdicts.rs`'s pin for `bug854315.pdf`'s unattributed signer — its assertion that the signature is no longer `NotChecked` is an expectation nobody has measured, and would fail on an MD5 digest, unusable coverage or a missing certificate; what the verdict makes of the corpus's seven timestamp tokens; and a re-run of `cms_census.rs`, whose pins (27 blobs parsed, 7 nested tokens) predate the strict decoding of RFC 2634's first-version `signingCertificate`, which fails a whole `ContentInfo` on a malformed one as the second version always did, and which RFC 3161 tokens normally carry | [features/signatures.md](features/signatures.md); [design/signatures.md](design/signatures.md) | the census's first run with these, recorded and pinned | S |
| **Public-key encryption.** **The content-cipher gap is closed**: `des-ede3-cbc` decrypts — the tables read twice from FIPS 46-3 and adjudicated by 500 NIST CAVP known answers — so the cipher OpenSSL still picks by default for older recipients now opens rather than being named. AES-192-CBC and RC2, which `cms -encrypt` can also be asked for and which no PDF has ever defaulted to, remain refused by name. What is left is the half the row always said was not work: the 7.6.5 key derivation is held only to a second implementation by the same author, and that risk closes the day a real public-key-encrypted document arrives. Writing landed on 2 October 2026 and OpenSSL opens what it seals, which adds interop evidence for the envelope and the cipher but not for the derivation; it also found a reader gap, recorded in the design doc's Risks: `/EncryptMetadata` is read from `/Encrypt` and not from the crypt filter, where Table 27 puts the public-key handler's. Re-measured 15 September 2026, and the clause survives — `/Adobe.PubSec` and `/Recipients` are each **zero across all 5 605 PDFs** under `corpus/files`; ten files match `pubsec` case-insensitively and every one of the ten is `/PubSec` inside a signature's `/Prop_Build`, a different key entirely | [features/encryption.md](features/encryption.md); [design/pubsec.md](design/pubsec.md), Risks; `des.rs` in `tinker-pdf-crypto` | a real public-key-encrypted document in a corpus | the risk is not work |
| **Non-device colour spaces on write: the CIE-based half.** **The ICC and tint halves have landed** — `add_icc_color_space`, `set_fill_icc` / `set_stroke_icc` and `ImageColorSpace::Icc` write 8.6.5.5's `/ICCBased` with the operand count taken from `/N`, and `add_separation_color_space`, `add_device_n_color_space`, `set_fill_tint` / `set_stroke_tint` and `ImageColorSpace::Tint` write 8.6.6.4's and 8.6.6.5's spaces with a type 2 or type 4 tint transform, held to the transform's own arithmetic pixel for pixel by `writer_tints.rs`. What is left is 8.6.5's CIE-based arrays — `/CalGray`, `/CalRGB` and `/Lab` — which this writer does not emit at all: no registration call, no setter, no image variant, and `creation.md` names the gap in its refusal table. Whether they are owed beside `/ICCBased`, which can carry what each of them says, is the owner's decision, and the refusal stays open until it is taken | [features/creation.md](features/creation.md); `add_icc_color_space` and `add_separation_color_space` in `crates/tinker-pdf-cos/src/build.rs` | a registered `/CalGray`, `/CalRGB` and `/Lab` space named by the fill and stroke setters and by `add_image`, read back through this reader and rendered to 8.6.5.2–8.6.5.4's arithmetic; or the owner's ruling that `/ICCBased` suffices, and the row moves to creation.md's refusal table as permanent | S |
| **Editing.** Redaction (September–October 2026) cuts a rotated or skewed run along its own baseline, a vertical run down its own column with the box placed by `/W2`'s whole position vector (`redact.rs`'s `vertical_runs`), and a Type 3 run in its own glyph space (`type3_glyph_space`); every appearance stream an annotation on the page can show, in the space 12.5.5 fits it into, inline images in it included (`appearance_streams`); a form drawn twice cut exactly at each placement, a copy each, within `MAX_FORM_COPY_BYTES`; and a Type 3 glyph at the use whose procedure draws under a rectangle, measured in the enclosing scope and in the font's own `/Resources` (`glyph_procedures`). What is left, and none of it is ruled permanent: **(a) glyph-procedure streams are not rewritten.** The exit asked for them rewritten; what was built instead removes the *use* and leaves the procedure, which `subset::apply` — the default save — empties once nothing shown runs it (`subset.rs`'s `type3_fonts`, which walks tiling cells and mask groups and counts a glyph whose procedure this engine could not run), so a procedure some uncovered use still runs keeps the covered words in `/CharProcs`. That substitute was this lane's choice, argued in `cut_stream`'s doc and the refusal table, and is the owner's to accept or refuse; until then this criterion is open. **(b) A tiling pattern's cell and a soft mask's group are named, not measured** (`RedactionWarning::PatternOrMask`, `patterns_and_masks`): a mask's group is drawn once at the `gs`'s transform and could be measured as one placement of a form; a cell is painted at every tile of what it fills, which is a design. **(c) Fonts and glyph procedures are read through the file**, not the editor (`fonts_in` over `DocumentEditor::document`, because `cos_font::from_resources` takes a `CosDocument`), so a run in a font the editor allocated — `import_page`, or font objects a caller allocated — is left whole as `UnknownFont`: named, and not "the page the editor has". **(d) Appearance synthesis draws thirteen subtypes and declines fourteen; the declining is not ruled.** October 2026 added `Line`, `Polygon`, `PolyLine`, `Squiggly`, `Caret`, `Ink` and `FreeText` beside `Highlight`, `Underline`, `StrikeOut`, `Square`, `Circle` and `Text` (`Link` draws none), and `/RD`, dashes and `/CA` for the first seven — whose bytes did not move only for a well-formed dictionary carrying none of `/CA`, `/ca`, `/RD`, a `/BS /S /D` or a `/Border` dash, which is what the editor's constructors make; with one they draw what it says, and with a malformed entry every subtype is declined. What is left of it, none of it ruled: **the fourteen subtypes `UNDETERMINED_SUBTYPES` names get no `/AP`** — the lane's position, argued in the feature doc's table, that no dictionary determines their appearance, which it does not apply evenly: `Text` is drawn as an invented note though 12.5.6.4 leaves its `/Name` icon to the reader exactly as 12.5.6.12, .15 and .16 leave `Stamp`'s, `FileAttachment`'s and `Sound`'s, and `Caret`'s outline is invented too, so whether those three get invented icons or an invented icon is refused (and `Text`'s with it) is the owner's; a free text annotation whose `/DA` names a composite font, left bare — a field's value is shaped in one (`fill.rs`'s `Composite`) and a free text annotation's is not — or a font `/DR` lacks, a symbolic one, or one whose encoding has no byte for a character; `/RC` rich text and `/DS` default style, drawn as plain `/Contents`; a line's caption (`/Cap`), a caret's paragraph symbol (`/Sy /P`), and a cloudy `/BE`, the beveled, inset and underline `/BS` styles and a `/Border`'s corner radii, drawn solid and square — each a row of the feature doc's refusal table. Permanent, and in the feature doc's refusal table: a run with no metrics or no finite frame, and the bounds `MAX_PLACEMENTS`, `MAX_FORM_COPY_BYTES` and 4 096 `Do`s a stream, each named | [features/editing.md](features/editing.md)'s redaction prose and refusal table; `crates/tinker-pdf-cos/src/appearance.rs` | (a) glyph-procedure streams rewritten, or the owner's ruling that removal at the use with `subset::apply`'s emptying suffices; (b) a mask group measured as a placement and a tiling cell measured, each with a fixture whose covered text is gone from every stream; (c) a run in a font only the editor holds cut, through `DocumentEditor::view` or a `Resolve`-generic font loader; (d) an `/AP` per further subtype, or the owner's ruling on which of the fourteen (and `Text`'s invented note) are refused; a free text annotation in a composite `/DA` font, and one with `/RC` and `/DS`, each given an `/AP` whose text renders where its layout puts it; the caption, the paragraph symbol and the border effects drawn, or ruled refused | (a) a decision, then M; (b) S for the mask, M for the cell; (c) M, in `tinker-pdf-cos`; (d) a decision, then S per subtype; S for the composite font, M for rich text |
| **Bindings.** Owed projections: the rest of `PageBuilder` (`encoded_text`, `glyphs`, `form`, `shading`, the two pattern setters, `set_ext_gstate`, `set_bleed_box`) and of `DocumentBuilder` (`with_version`, `add_named_font`, `add_cid_font`, `glyph_run`, `add_ext_gstate`, `add_form`, `add_shading`, `add_tiling_pattern`, `clear_image_resources`); `DocumentEditor`'s `import_page`, `keep_pages`, `flatten_annotations`, `add_annotation`, `reset_form`, `set_field_values`, `set_calculated_values`, and the document operations — `set_page_labels`, `attach_file`, `set_outline`, the typed `/Info` setters, `set_xmp_metadata`, `set_viewer_preferences`, `set_page_boundary` and its three siblings, `sanitise` with its report; `WriteOptions::deduplicate_streams` (the C ABI builds it at its default); the September 2026 graphics-writing surface — `Target::Named` and both `add_named_destination`s, `DocumentBuilder::add_layer` with `PageBuilder::optional` and `DocumentEditor::set_layer_visible`, `add_separation_color_space` and `add_device_n_color_space` with `Function::Calculator` and `DeviceNAttributes`, `set_fill_tint` / `set_stroke_tint` and `ImageColorSpace::Tint`, `DocumentEditor`'s `stamp`, `add_resource`, `add_form` and `import_page_as_form`, and `Page::images` with `PageImage`; the forms surface — `DocumentEditor::add_field` and `NewField`, `form_data`'s FDF and XFDF import and export, `SigningTarget::NewVisibleField` with `SignatureAppearance`, and the annotation payloads with `Page::annotation_list`; `PageBuilder::tagged` as an `open_tag`/`close_tag` pair; a signature's `/Contents`, `/M`, `/ContactInfo`, `/Filter`, its warnings and `modifications`; signatures in Python and JavaScript; the read surface — outline, links, metadata, attachments, XMP, warnings; `authenticate_with_recipient`; the October 2026 signature and public-key surface — `DocumentEditor::save_timestamped` with `Timestamper` and `TimestampRequest`, `add_validation_data` with `ValidationData`, `Document::security_store` with `SecurityStore` and its warnings, `PublicKeyEncryption::seal` with `EntropySource` and `DocumentEditor::save_sealed`, `Verdict::timestamps` with `TimestampVerdict`, and `Signature::validation_key` | [features/bindings.md](features/bindings.md); the C ABI exports 123 functions today | each through the C ABI and the three wrappers with a parity-script line; `cargo xtask bindings-parity` green | L ([design/bindings-write.md](design/bindings-write.md) covers the write half; the read half is owed a section there) |
| Published packages: `pip install`, `npm install` and `dotnet add package` do not work, by decision, until the facade freezes at 0.1.0 | [features/bindings.md](features/bindings.md); the release pipeline has run once and published nothing | a `workflow_dispatch` with `publish: true` | a decision, then S |
| PDF 2.0 deltas still "tracked": the 2.0 blend and dash clarifications | [pdf20-deltas.md](pdf20-deltas.md). **The blend and dash half cannot be read here**: on 26 September 2026 the sponsored ISO 32000-2 copy at pdfa.org was refused by the egress policy (403), and the two reachable PDF Association sources — the `pdf-issues` errata and the Arlington model — carry no 8.4.3.6 erratum and nothing about 11.3.5 beyond a NOTE on its figures; pdf20-deltas.md lists what each said. The errata *do* quote Table 145's `/CS` text excluding Lab from a transparency group's colour space, so the `/Lab` group `features/rendering.md` composites in Lab is a file the specification does not permit — what to do with one is a decision, not a delta, and is not taken here | each row moves to a status with a doc behind it | S each |

**Decision items, not commitments.** Each is a choice recorded so it is not an
omission, and each waits on evidence rather than on time: **OCR**, if ever, as
a host seam like `FontProvider` and not an engine; **container writing** — CBZ,
XPS and EPUB are read-only conversions; **an archival profile on
`WriteOptions`** — `rewrite` returns `Vec<u8>` with nowhere to put a refusal,
which is why the builder has one and the rewriter does not
([design/pdfa.md](design/pdfa.md)); **image encoders the *writer* reaches for**
— `tinker-pdf-filters` writes PNG, CCITT G4 and JBIG2 generic regions as of
15 September 2026 and baseline JPEG as of 16 September, and the document writer
calls them only when a save's `SaveOptions::images` names one, so partial image
redaction still waits on the decision of when to choose a codec rather than on
a codec; **rendering intents** — measured and declined, since
no corpus file sets an ExtGState `/RenderingIntent` and five carry `ri`
([design/icc.md](design/icc.md)); **variation-aware, vertical, AAT and
Graphite shaping, and `JSTF`** — deferred under ruling 3
([design/shaping.md](design/shaping.md)); **forms** — the five catalog actions
(`WC`, `WS`, `DS`, `WP`, `DP`) that nothing runs, and a narrower production
than `NotADefinition` for provably inert document-scope statements, both wait
on a corpus count ([design/form-script-policy.md](design/form-script-policy.md)).

## Tier 4 — container depth

Debts each container format records about itself, in its feature doc's
refusal table.

| Item | Evidence | Exit criterion | Size |
| --- | --- | --- | --- |
| **EPUB CSS: forty-three properties known and unimplemented**, each reported as `UnimplementedProperty` with an element count, against the 132 names `IMPLEMENTED_NAMES` carries, longhands, shorthands and aliases alike. The ones that change a paged output: `background-attachment`, `list-style-image`, `writing-mode`, `direction`, `unicode-bidi`, `hyphens`, `page`, `clip-path`, `filter`, `mix-blend-mode`, `grid*`, `font-feature-settings`, `font-kerning`; and custom properties, which none of the committed books declares. **`break-before`, `break-after` and `break-inside` have left this row** (3 October 2026) as `css-break-3` §3.4's aliases of the `page-break-*` longhands, held by three pairs in `epub_reftest.rs`; `column`, `avoid-column`, `region`, `avoid-region`, `recto` and `verso` are refused by value. **`text-transform` has left it** (3 October 2026): Unicode's full case mappings from UCD 17.0.0 vendored beside the line breaker, Final_Sigma included, held by `uppercase_is_the_text_written_in_capitals_and_is_measured_so` and `lowercase_and_capitalize_are_the_text_written_that_way`; the language-conditional mappings (`lt`, `tr`, `az`) and `full-width`/`full-size-kana` are still counted against it. **`list-style`, `list-style-position`, `counter-reset` and `counter-increment` have left it**, with `counter-set`, which no list here named (3 October 2026): `css-lists-3` §4's counter tree walked after the cascade, HTML's list attributes as presentational hints, `inside` markers, and `counter()`/`counters()` in `content` — held by `an_inside_marker_is_a_before_box_holding_the_counter`, `the_list_style_shorthand_is_its_longhands`, `ol_start_and_li_value_are_counter_reset_and_counter_set` and `nested_lists_number_through_the_counter_tree`; `reversed()` and `<ol reversed>` are still counted against `counter-reset`. **`quotes` has left it** with the four quote keywords (3 October 2026), held by `a_q_element_is_its_text_between_the_marks_quotes_names`; `quotes: auto`, the initial value, is still counted against it wherever a quote keyword meets it, since its marks are HTML §15.3.6's per-language table, which is not vendored. **`opacity` has left it** (3 October 2026) as a per-fragment `/ExtGState` alpha composed down the element tree, held by `epub_paint.rs`'s operator-level tests (`opacity_is_an_alpha_on_the_elements_fragments`, `nested_opacities_multiply`, `opacity_over_a_painted_box_with_content_is_counted`); the transparency group §15.1 composites, which differs where content inside the element overlaps its own background, is **owed to the structure writer** — it needs tagged marked content inside a form XObject — and those elements are counted against `opacity` meanwhile. **`border-radius` and `outline` have left it** with their longhands (3 October 2026): elliptical corners with §5.5's overlap scaling and per-side rings, asserted on the cubics' control points against the closed form (`a_rounded_corner_is_the_quarter_arc_cubic`, `overlapping_radii_are_scaled_by_one_factor`, `a_slash_radius_is_an_elliptical_corner`, `a_rounded_border_is_a_ring_per_side`, `a_rounded_box_cut_across_pages_rounds_only_its_real_ends`), and outlines as bands outside the border edge after the text (`an_outline_is_drawn_outside_the_border_edge_after_the_text`). **`overflow` has left it** with `overflow-x` and `overflow-y` (3 October 2026): §3.1's computed value, a clip to the padding box written only where the content reaches past it, scroll containers as block formatting contexts — margins kept inside, floats contained, cleared below a float beside them — and the content past a block-axis clip taken out of the column and kept as text laid out and not painted, so conservation still holds; held by `epub_paint.rs`'s `an_overflowing_box_clips_its_content_to_its_padding_box`, `a_box_whose_content_fits_writes_no_clip`, `a_rounded_box_clips_to_its_padding_edges_curve`, `a_positioned_box_is_clipped_only_through_its_containing_block` and `an_elements_clip_cuts_its_own_text`, and by three pairs in `epub_reftest.rs` (`a_scroll_container_contains_its_floats_as_a_clearing_block_does`, `a_scroll_containers_first_childs_margin_stays_inside_it`, `overflow_on_body_is_the_pages_and_not_the_bodys`); `scroll` and `auto` print as `hidden`, and a scroll container beside a float is cleared rather than narrowed, both stated in the refusal table. **The `background-*` image family has left it** — `background-image`, `background-repeat`, `background-position` and `background-size`, with the `background` shorthand now setting all five (3 October 2026): one raster `url()` resolved against its sheet, read after layout for the fragments that reached a page, positioned in the padding box and clipped to the border box, one placement or a tiling pattern; held by `epub_images.rs`'s `a_background_image_that_does_not_repeat_is_drawn_once_where_it_is_placed`, `cover_and_contain_scale_the_image_by_its_own_ratio`, `a_repeating_background_image_is_a_tiling_pattern`, `space_and_round_fit_whole_images` and `a_background_url_is_relative_to_its_sheet_and_a_missing_one_is_named`; gradients, a second layer and `background-attachment` are still counted. **`box-shadow` and `text-shadow` have left it** (3 October 2026), hard-edged: an outer box shadow is the border box offset and grown by the spread, its corners by §7.1.1's `r + s(1 + (r/s − 1)³)`, under the background and clipped to outside the border box; an inset one is the band between the padding box and that box offset and shrunk, over the background and under the border; a text shadow is the run drawn again under the text as an artifact, so it is read once; a translucent colour is its alpha times the element's composed opacity — held by `epub_paint.rs`'s `a_box_shadow_is_the_border_box_offset_and_spread_outside_the_box`, `a_shadows_corners_grow_by_the_spread_and_a_small_one_by_less`, `an_inset_shadow_is_drawn_inside_the_padding_box_over_the_background`, `a_text_shadow_is_the_run_again_under_the_text_and_is_not_read_twice`, `a_translucent_shadow_is_its_alpha_times_the_elements_opacity` and `a_blurred_shadow_is_counted_and_not_drawn`; **a blur is refused by value and still counted**, as is a list longer than the new `MAX_CSS_SHADOWS` (32). **`transform` and `transform-origin` have left it** (3 October 2026), two-dimensional: §13.1's functions as one `cm` per transformed element, about its origin in its border box on the page, composed outermost first with the clips between them, through the SVG crate's deterministic sine (ruling 4); a repeating background's pattern matrix carries the transform, since no `cm` reaches a pattern; a matrix with no inverse draws nothing; and a transformed box is the containing block of its absolutely positioned descendants — held by `epub_paint.rs`'s `a_rotation_is_a_cm_about_the_border_boxs_centre`, `the_list_applies_rightmost_first_about_the_origin`, `a_nested_transform_composes_inside_its_ancestors` and `a_transform_with_no_inverse_draws_nothing`, `epub_images.rs`'s `a_transformed_boxs_repeating_image_carries_the_transform_in_its_pattern` and `epub_reftest.rs`'s `a_transformed_box_contains_its_absolute_descendants_as_a_positioned_one_does`; **the 3D functions are refused by value and still counted**, as are a `position: fixed` box under a transform (laid out against the page) and a link under one (its active area is untransformed) | `UNSUPPORTED_PROPERTIES` in `crates/tinker-pdf-css/src/property.rs`, counted. Milestone 1's census measured 84 distinct names across the fetched corpus's 53 stylesheets and 42 across the committed 8; **the committed figure is re-measured** by `epub_css.rs`'s `the_committed_stylesheets_write_this_many_distinct_property_names` — 42 over the first six books' eight sheets, 44 over all nine books' twelve — and the fetched figure is owed, since the fetched corpus is not in this tree. Against the committed nine, the unimplemented names reach `color-scheme` 19 elements and `hyphens` 18 (`CENSUS.tsv`, re-measured 3 October 2026); `overflow` (6) and `overflow-x` (3) left with `overflow`, and `list-style` and `quotes`, each declared four times on no element any book has, left before them | scheduled by the fetched corpus's `UnimplementedProperty` counts, highest first — **owed**: that ordering cannot be measured here, so the landings since 3 October 2026 are in the order the row's own list and the committed counts gave; each landing deletes its name from the table | L — [design/epub-layout.md](design/epub-layout.md), written with the replaced box and carrying the slices as its milestones 2 to 7 |
| EPUB layout refusals, each a typed warning in `tinker-pdf-layout`. **What is left**, re-read 3 October 2026: `max-height` shorter than its content on a box whose `overflow-y` is `visible`, treated as `auto` (`MaxHeightAsAuto`) — the content past the used height would have to be drawn over the box after it, which a column whose `y` never goes back cannot place, and a straddling child would be painted short; an atomic box taller than a page inside a table band, a flex line or a column, drawn where it starts; `column-span: all` on a box **below** a multi-column container's own children, laid out in its column (`ColumnSpanAsNone`); an absolutely positioned block inside an inline box, set in the line (`BlockInInline`); a table column's background **image** (`ColumnBoxNotPainted`); `::first-line`, parsed with no box (it needs a second layout pass); `content: url()`, generating nothing (a replaced box inside a generated one, and the painter keys a picture by its element, so `::before` and `::after` would share one); and `@page`, skipped — **its size and margins wait on the owner**: the page box is the caller's `OpenOptions::page` and `PAGE_MARGIN`, and eight of the nine committed books write an `@page` margin — pandoc's four `margin: 10px`, calibre's three `margin-top: 5pt; margin-bottom: 5pt` (one also `margin: 0pt` on its title page), the fixed-layout comic `margin: 0` — so honouring a book's `@page` over the caller's box changes nearly every page here and is a decision about who owns the page, not a property to implement. **Left this row on 3 October 2026**, each held by a pair in `epub_reftest.rs` with its mismatch reference: `column-span: all` on a child (`a_spanning_child_is_the_column_sets_either_side_of_a_block`, `css-multicol-1` §6's interruption: a balanced column set either side, the spanner a block between); `max-height` on a box that clips (`max_height_on_a_clipping_box_is_the_height_it_clamps_to`: a `max-height` over taller content in an `overflow: hidden` box is that `height`; `tinker-pdf-layout`'s `a_block_axis_clip_drops_the_content_past_the_used_height` holds the clip itself); `inline-flex` (`inline_flex_is_an_atomic_inline_holding_a_flex_layout`: an atomic inline whose inside is a flex layout, its baseline §8.5's first; `InlineFlexAsBlock` is gone); a block inside an inline (`a_block_inside_an_inline_splits_it_into_anonymous_blocks`: CSS 2.2 §9.2.1.1's split of every inline ancestor round an in-flow block — it had been poured into the line); a table column's background colour and border (`a_column_background_is_its_cells_backgrounds_under_the_row_group`, a render pair: §17.5.1's column layers under each cell that originates in them, skipped under an opaque layer so no seam is left; a column's border resolved in the collapsing model and ignored in the separated one, as §17.6.1 says); anonymous row generation (`cells_with_no_row_are_the_row_the_markup_omitted`: §17.2.1 rule 8's anonymous row of the cells themselves — it had been one anonymous cell holding them all); `::first-letter` (`first_letter_is_the_first_letter_in_a_box_of_its_own`: `css-pseudo-4` §2.2's letter unit wrapped while the box tree is built, through inline boxes and into a first child block, a floated one a drop cap; it inherits from the originating block rather than an inline box round the letter, a letter an element boundary separates from its opening quotation mark is not found, and of nested containers' only the innermost makes a box — `a_nested_first_letter_is_the_nearest_blocks`); the quote keywords and `quotes` (`a_q_element_is_its_text_between_the_marks_quotes_names`; under `quotes: auto` they draw nothing and are counted against `quotes`); `counter()` and `counters()` (`nested_lists_number_through_the_counter_tree`, and `epub_pseudo.rs`'s `counter_and_counters_number_the_generated_boxes`; a counter style outside the nine this build formats is refused by value); `:nth-child(An+B of S)` (`nth_child_of_a_selector_is_the_position_among_its_matches`: `selectors-4` §14.4.1 for `:nth-child()` and `:nth-last-child()`); and `@supports` (`a_supports_block_is_its_rules_where_the_test_is_supported`: `css-conditional-3` §6 evaluated against what this build implements, with `not`/`and`/`or` and `css-conditional-4`'s `selector()`) | [features/epub.md](features/epub.md) | each row's warning stops firing on a reftest pair in `epub_reftest.rs` | S–M each |
| EPUB text set in a standard-14 fallback face loses characters past a simple font's 256 codes — 224 outside `WinAnsiEncoding` — counted as `UnrepresentedCharacters`. **Verified: an embedded face is exempt.** `epub/paint.rs` returns on `Chosen::Embedded` before any counting, and only `Chosen::Standard` reaches the counter, so this is the standard-14 fallback alone | `crates/tinker-pdf/src/epub.rs` pushes the warning | a CID-keyed path for fallback text; the conservation harness unchanged | M |
| EPUB features tallied and not implemented: MathML layout (one fetched book has 71 `mathml` items), media overlays, `switch`, `remote-resources`; and a font provider attached after open cannot re-paginate, since advances decided the line breaks at open | [features/epub.md](features/epub.md) | MathML and overlays are decisions to take against corpus counts; re-pagination is a design question before it is work | decisions |
| EPUB corpus gaps: no committed stylesheet declares a cascade layer, so `@layer` is verified against this engine's own reading of css-cascade-5 and against no producer; no producer book carries a `.woff` or `.woff2` in its ZIP, so the `@font-face` path is held to containers this repository packs | [features/epub.md](features/epub.md); nine committed books | a real producer's book for each | S, fixture hunting |
| **SVG in the spine**: at-rules inside `<style>` — `@media`, `@import`, `@font-face` — skipped by CSS's own recovery and named (`AtRuleIgnored`); a `mask`, `clip-path`, gradient or pattern in `objectBoundingBox` units on text — whose box is its glyph cells, a font metric the leaf does not have (ruling 8) — drawn without the effect, or in the paint's own fallback, and named (`TextBoxUnmeasured`; `text_under_a_bounding_box_mask_draws_unmasked_and_is_named` and three siblings): *corrected on review, 3 October 2026*, because `<mask>` and `<pattern>` left this row with text under them masked away or painted `none` without a word, where before them it had been drawn and named; and **a measurement owed**: `epub_fetched.rs`'s `the_six_svg_spine_items_draw_rather_than_placehold` asserts that no marker reference in the fetched corpus's thirty-two goes unresolved, and could not be run here because the corpus cannot be fetched in this container. **Left this row since the milestones**, each with its tests and its account in [design/svg.md](design/svg.md)'s *As built*: a group's `opacity` and `clip-path` as one transparency group (`a_groups_opacity_is_composited_once`); `<marker>` at its vertices (`tests/markers.rs`); `spreadMethod` `reflect` and `repeat` as one calculator per shading (`a_repeated_gradient_starts_again_every_period`); the per-glyph `x`/`y`/`dx`/`dy`/`rotate` lists (`an_x_per_character_sets_each_where_it_says`); `<mask>` as a luminance soft mask (`tests/masks.rs`); `<pattern>` as an 8.7.3 tiling pattern (`tests/patterns.rs`); and a clip path's `<use>` and `<text>` children, text as a mask of its silhouettes (`tests/clip_children.rs`). Writing them found six defects that drew a plausible page: a `<g>`'s and an `<image>`'s `clip-path` dropped without a word; a clip's `<use>` and `<text>` children skipped without one, though this row called them named; a `y` with no `x` and a continuing `dx` each setting text in the wrong place; a shading's one sampled or calculator function read as the identity (`shading_functions.rs`); a group under a soft mask masked twice (`soft_mask_clips.rs`); and a mask's or a tile's text naming a font the file did not hold. **Kept as named refusals, each a decision** recorded under *Named non-goals*: `<filter>`, `<foreignObject>`, SMIL and `<script>`. **`<textPath>`, `<tref>` and `<altGlyph>` stay in this row**, each named (`TextLayoutUnsupported`) and **unscheduled**: no file here holds one, and under ruling 3 that leaves them waiting on a count, not decided against. *Corrected on review, 3 October 2026*: they had been moved to the decisions with "none has a file behind it" as the reason — a scheduling fact ruling 3 does not let stand as a permanent refusal, which is the move the TIFF row's compression 6 had reversed the day before | `crates/tinker-pdf-svg/src/lib.rs`; [design/svg.md](design/svg.md) | the at-rules read (`@media` evaluated as print, `@font-face` through the container), or ruled a decision; a run's box measured by the caller, which has the metrics, and handed to the leaf; `<textPath>` and its two relatives drawn when a corpus count asks for them, or ruled a decision by the owner; the corpus measurement taken where the corpus is | S each; M for text's box |
| **XPS.** A `ContextColor` gradient stop whose profile this build cannot evaluate — a CMYK press profile with no `A2B*` table, an `mAB ` one — keeps 8.6.5.5's alternate reading and is `BrushApproximated`; and every fixture the rows below left with is **derived** (`tests/xps_rows`, WPF's package with its page replaced), because no producer on hand writes these features, so a producer's package for each is owed | [features/xps.md](features/xps.md) | the evaluable profile set grown as `tinker-pdf-color`'s is; a producer's package per feature in the sweep | S each, and a producer to find. **A `ContextColor` gradient stop has left this row**: it is converted to sRGB through its profile — 18.3.1.2's own first step, and this build's choice where every stop is a `ContextColor`, since no print ticket's blending space is read — then blended in the brush's `ColorInterpolationMode` (`a_context_colour_in_a_gradient_stop_is_converted_through_its_profile`), and `tests/xps_rows/wpf-context-stops.xps` conserves 3 facts of 3, its census evaluating the grey profile itself from ICC.1. **`StyleSimulations` has left this row**: ECMA-388's text was read on 3 October 2026 (12.1.5, M5.12–M5.14, S5.6), and the four values are drawn as it states them — a 2% stroke in the fill's paint, font advances widened by 2% and the glyphs moved 1% up and right, a 20° shear — with `tests/xps_rows/wpf-style-simulations.xps` conserving 5 facts of 5 and rendered heavier and leaning (`the_simulations_draw_heavier_and_leaning_ink_in_a_real_font`); a value 12.1.5 does not name is still `GlyphsStyleSimulated`. **Per-stop alpha has left this row**: a gradient whose stops' alphas differ paints its alphas as a second `/DeviceGray` shading read back as a `/Luminosity` soft mask, in a group of its own so an `OpacityMask` composes with it, on fills, strokes and glyph runs (`gradient_stops_with_different_alphas_fade_across_the_element`, `stop_alphas_compose_with_the_elements_own_mask`), and `tests/xps_rows/wpf-stop-alphas.xps` conserves 4 facts of 4. **`ColorInterpolationMode` has left this row**: `ScRgbLinearInterpolation` blends each interval in linear light, written as a one-input `Function::Sampled` of the sRGB it comes to (`sc_rgb_interpolation_blends_in_linear_light`), and the census compares every interval's middle, so `tests/xps_rows/wpf-colour-interpolation.xps` conserving 6 facts of 6 is a statement about the blend and not only the stops; a value 18.3.1.2 does not name is blended as the default and named. **`nCLR` has left this row**: a profile of a channel count `/ICCBased` cannot state — `2CLR`, and `5CLR` to `8CLR` — is an 8.6.6.5 `/DeviceN` whose tint transform is `tinker-pdf-color`'s transform run over a grid and written as a sampled function into `/DeviceRGB` (`an_n_channel_profile_is_a_device_n_whose_tint_is_the_profile`), and `tests/xps_rows/wpf-n-channel.xps` conserves 5 facts of 5; it needed this repository's reader to interpolate a sampled function across all its inputs, which it did not (`shading_functions.rs`). Past eight channels — ECMA-388 names no more — or with no table to evaluate, it is still `ColourProfileChannels`. `3CLR` and `4CLR` have counts Table 66 admits and stay `/ICCBased` under `/N 3` and `/N 4`, as they always were (`a_three_or_four_channel_n_clr_profile_stays_icc_based`); *corrected on review, 3 October 2026*, where this row said every two- to eight-channel profile was a `/DeviceN`. **`{ColorConvertedBitmap}` has left this row**: the picture is drawn and its profile embedded as an `/ICCBased` space, and `gs-images.xps` joined the conservation sweep with it. **Interleaved OPC packages have left it too**: pieces are joined by `xps::opc`, and `tests/xps_interleaved/wpf-image-and-text-pieces.xps` — a WPF package cut into pieces — conserves 3 facts of 3 and renders byte-identically to the package it was cut from |
| **Archives.** AVIF needs a codec | [features/cbz.md](features/cbz.md); every CBZ fixture is in the tree, so there is no corpus count. **Zstandard has left this row**, as ZIP method 93: a hand-rolled RFC 8878 decoder in `tinker-pdf-archive` — FSE and Huffman tables, sequences, repeat offsets, window limits, the XXH64 content checksum — held to RFC 8878's Appendix A state by state, to the zstd project's own golden files (`the_golden_files_decode_to_what_zstd_s_own_tests_say`, `the_golden_error_files_are_refused_for_their_reasons`), and to libzstd 1.5.7's frames over files made here, whose entries decode to the files that went in (`libzstd_s_frames_are_the_files_that_went_in`, `libzstd_s_method_93_entries_are_the_files_that_went_in`); a frame that names a dictionary is refused by name. **7z's BCJ2 folder (`0303011B`) has left this row**: the folder walk is a tree walk now, over coders with several inputs and pack streams in any order, and 7-Zip 26.02's own Linux build (`7zz -m0=BCJ2 -m1=LZMA -m2=LZMA -m3=LZMA`) wrote `7zz-bcj2.7z` over `x86.bin`, whose entries decode to the files that went in (`seven_zip_s_bcj2_entries_are_the_files_that_went_in`) with calls and jumps converted in both target streams. **7z's PPMd coder (`030401`) has left this row**, transcribed from 7-Zip's public-domain `Ppmd7.c` and held to py7zr's archives over files made here — order 6 in 16 MiB, and order 32 in 64 KiB, whose model restarts again and again — every entry decoding to the file that went in (`py7zr_s_ppmd_entries_are_the_files_that_went_in`). **bzip2 has left this row**, as ZIP method 12 and 7z coder `040202`, a hand-rolled decoder in `tinker-pdf-archive`: CPython's `zipfile` and py7zr, both over libbzip2 1.0.8, wrote archives over files made here — two blocks, six Huffman groups, every run-length threshold, an empty stream — and every entry decodes to the file that went in (`cpython_s_method_12_streams_are_the_files_that_went_in`, `py7zr_s_bzip2_entries_are_the_files_that_went_in`); randomised blocks, unwritten since 1999, are refused by name. **7z's BCJ filter (`03030103`) has left this row**, its fixture arriving with its decoder: py7zr's `FILTER_X86` in front of LZMA2 over `tinker-pdf-archive/tests/coders/input/x86.bin`, a file shaped to reach every branch of the filter, decodes to that file byte for byte (`py7zr_s_bcj_entries_are_the_files_that_went_in`), and `py7zr-bcj.cb7` is the second 7z writer the comic corpus lacked. **ZIP method 14 and JPEG 2000 have left this row**, the two decoders that already existed: `Archive::read_with` takes the LZMA decoder 7z already had, and CPython's `python-lzma.cbz` decodes to the five committed source files byte for byte; a `.jp2` or `.j2k` page is placed as `/JPXDecode` with its own bytes, and `python-jpx.cbz` renders to T.800 J.10.5's published samples **BMP and GIF have left this row** (26 September 2026): `bmp_decode` reads every header layout, depth and coding Win32 documents and `gif_decode` a GIF's first image, interlaced or not, on any root size; a page of either is decoded and kept `/Indexed` where the file was, an EPUB `<img>` draws a GIF, and `image_fixtures.rs` holds both to pixels Pillow, imagecodecs and omggif were handed, plus bmpsuite's relations. **WebP has left this row**: the lossless half on 26 September 2026 — `webp_decode` reads RFC 9649's container and its VP8L bitstream, every transform, meta prefix codes, back-references and the colour cache, held by `image_fixtures.rs` to pixels libwebp was handed through Pillow and imagecodecs — and the lossy half on 2 October 2026: RFC 6386's key frame and the `ALPH` chunk beside it, its planes held to the WebM project's published test vectors (182 key frames of 61 files, 0 failed; the vectors carry no licence, so `tests/vp8-vectors/fetch.sh` fetches them pinned by commit and SHA-256 and CI's `vp8-vectors` job runs them with `TINKER_VP8_VECTORS_REQUIRED=1` and greps its `RAN` banner), its RGB conversion to BT.601 and its upsampler to its 9:3:3:1 weights, its alpha exactly and its colour within a stated error to the pictures the encoder was handed. *Corrected on review the same day*: the lossy files were first held to libwebp's own decode of them, which ruling 13 forbids; nothing takes libwebp's output as an answer now. A page of either is decoded, an EPUB `<img>` draws either, and an animation is its first frame | by evidence | L for AVIF |
| TIFF, reached from XPS and CBZ: `PhotometricInterpretation` 8 (CIELab). It needs either PDF's `/Lab` space with the `/Decode` and `/Range` TIFF's signed a*/b* bytes ask for, which the embed door does not carry, or a conversion — and the colour crate's `lab_to_srgb` is a floating-point transform, not the exact mapping this row was allowed to use. And `Compression` 6 (old-style JPEG), which Technical Note 2 replaced: a file whose `JPEGInterchangeFormat` (tag 513) points at a complete interchange-format stream is readable by the existing JPEG decoder without a guess; one that carries only TIFF 6.0 §22's per-component tags is not | [features/filters.md](features/filters.md); no count recorded for either. **Left this row 26 September 2026**: CMYK (5), `SampleFormat` 2 and 3 with a stated mapping, `Predictor` 3, BigTIFF, `Compression` 34712, and directories after the first (a CBZ pages each); `image_fixtures.rs` and `cbz_images.rs` hold each to tifffile's files from authored pixels. 4 (a mask) and 32803 (a CFA) moved to filters.md's refusal table as permanent; compression 6 was moved with them and is back here, corrected on review (2 October 2026): a permanent refusal with no count recorded was a scheduling decision ruling 3 does not allow | by evidence | S, unscheduled |
| JPEG XR's named refusals: fixed-point, half-float and float pixel formats; the packed sub-byte depths; CMYK, CMYKDIRECT, NCOMPONENT and RGBE; an unknown pixel-format GUID; the interleaved alpha plane; YUV420, YUV422 and YUVK; a windowed origin | [features/filters.md](features/filters.md), each reached by `jxr/tests/refusals.rs`; the platform encoder cannot emit most of them | a fixture from a real encoder per row | unscheduled |
| Fonts on write: Symbol and ZapfDingbats have no bundled equivalent and are unreadable when nothing embeds them, and **stay so** while no face with their repertoire is under a licence `deny.toml` admits — URW's base-35 stand-ins, Standard Symbols PS and D050000L, are AGPL-3.0 with a font exception, and Liberation has neither (`deny.toml` read again 3 October 2026; a host's `FontProvider` may supply one). **WOFF2's table transforms have left this row** (3 October 2026): checked against the W3C Recommendation of 8 August 2024, `transform_kind` reverses every transform clause 5 defines — `glyf` and `loca` version 0, `hmtx` version 1, and the three null versions — and the versions it refuses are the ones §4.1 says "the entire font MUST be rejected" for. Reading it found §5.3's pairing unkept: a transformed `glyf` with a null or missing `loca` decoded, and is refused now (`a_transformed_glyf_needs_its_loca_transformed_with_it`). **A CID-keyed CFF under `add_cid_font` has left this row** (3 October 2026): each glyph is written as the CID its charset gives it, in the string, `/W` and `/ToUnicode` (9.7.4.2; `cff_subsetting.rs`, `a_cid_keyed_program_the_writer_embedded_draws_the_glyph_it_was_given`), and a charset that is not one-to-one is refused by name in [features/fonts.md](features/fonts.md). Writing it found an `OpenType/CFF` face written as a CIDFontType2 with `/CIDToGIDMap /Identity`, which 9.9 Table 126 rules out over a `CFF ` table; it is a CIDFontType0 now | [features/fonts.md](features/fonts.md) | by evidence | S each |

## Tier 5 — capabilities the field expects

Every row here was checked absent or partial in source on 4 September 2026,
by grep against the facade, the two write surfaces, the CLI and the four
bindings, never against a doc alone. None has corpus evidence, because a
capability an engine lacks leaves no trace in a corpus run; the groups are
ordered by how much of the field uses them, which is a judgement and is
labelled as one. **Every L and XL row is owed a design doc before it is
scheduled.** The four L rows this tier carries — inferred reading order,
table reconstruction, PDF/UA validation, PDF/X validation and writing — got
theirs on 16 September 2026, each written before its row was scheduled and
each naming what decides whether the build is right; **none of the four is
scheduled**, and the PDF/X document says in as many words which part of its
row cannot be. The two rows that price an L or XL alternative (hinting, a full
ECMAScript engine) are decisions rather than rows and owe nothing until they
are taken. Where a row reverses a non-goal a feature or design doc states, the
reversal is a decision taken here and the doc changes in the commit that
schedules it.

### Output

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| ~~Image encoder: JPEG~~ **All three image encoders are written and have a caller. One named gap is left, and it is not a coder** | **The last of the three landed 16 September 2026.** `tinker_pdf_filters::jpeg_encode` writes ITU-T T.81's **baseline** process and only that: sequential DCT, Huffman, 8-bit, one SOF0 frame, one scan, `Ss = 0`, `Se = 63`, `Ah = Al = 0`. Grayscale, or T.871 clause 7's YCbCr with the JFIF APP0 that clause 10.1 requires so a reader is told which YCbCr it is; 4:4:4 or 4:2:0 **named by the caller and never inferred from the pixels**; Annex K's quantisation tables, those tables halved (K.1's own second paragraph is the only other setting T.81 names) or the caller's own, with **no 1-to-100 quality number** — every such scale in circulation is some program's private convention and none is in T.81, T.83 or T.871, so the caller who wants a specific quality supplies the table; A.2.4's partial MCUs completed by its NOTE's replication of the right-most column and bottom line; an optional restart interval. Progressive and extended sequential, arithmetic, lossless, hierarchical, 12-bit, four-component CMYK/YCCK and K.2's optimised tables are excluded **by name** in the module header, each with its reason. **`tinker-pdf-cos` imports no encoder**, and the writer still never re-encodes image bytes, which is a contract and not a missing coder; the facade calls all three — `png_encode` inside `Bitmap::to_png`, and `jpeg_encode`, `ccitt_g4_encode` and `jbig2_generic_region_segment` from the save door's image pass, **only when `SaveOptions::images` names one** (the Document operations row). PNG stays the one image format the facade projects — `Bitmap::to_png`, `tpdf render` and `png_encode` | **The coder is done, and so is a caller; one thing is not, and it is the row's own exit criterion failing.** (1) *A caller* — **landed as a recoding pass on save** (the Document operations row), and the policy question it needed is answered by not answering it: the caller names the coding per image kind and supplies the JPEG tables, and the pass keeps a result only when it is smaller. The builder still deflates every raster it is handed (creation.md). (2) **No published DCT vector set adjudicates the coefficients this produces.** ITU-T T.83 (ISO/IEC 10918-2) is the compliance data published for exactly this purpose, and its own clause 4.4 says the data ships *on three diskettes* accompanying the document rather than inside it; the ITU's copy returned HTTP 500 on 15 and again on 16 September 2026 and the Recommendation's own page says it "is only available through payment", ISO's returns HTTP 403, and the one reachable copy is the standards-preview extract — read here with `tpdf`, it carries the numbered pages 1 to 11 and stops mid-sentence in clause 5.2.1, where its own contents list puts clause 6's encoder compliance tests on p. 19, Annex B's compliance quantisation tables on p. 28 and Annex C's compressed test data on p. 30. The Internet Archive holds nothing: no full-text hit, a 429 from the availability API and a 404 on a direct Wayback fetch. So the FDCT is held to **A.3.3's equation recomputed in `f64` in the test** — the standard's formula, not the standard's numbers — and the row says so rather than letting a round trip stand in. What *is* adjudicated by published data: Figure A.6's zig-zag read twice, Tables K.1 and K.2 read twice, K.3.3's four DHT byte lists appearing verbatim in the output, and two whole entropy-coded segments derived from Tables K.3's and K.5's printed code words with no implementation in the middle | closed for the coder and for a caller; fixture hunting for T.83, which may never end |
| A tile byte-equal to the page when an image run's overlap falls outside it | Ruling 5's guard (`crates/tinker-pdf/tests/render_regions.rs`) holds sixteen fixtures — text, shadings, an image, strokes, groups, soft masks, a tiling lattice and a mesh, turned and cropped — byte-equal tile by tile at 0.5× to 4× and down to a one-pixel lattice, since the tile stopped having a frame of its own (26 September 2026). **One decision is still the canvas's**: whether an image joins the run held back so that abutting images do not conflate is decided by overlap with the fragments the run holds over the canvas only. Two images that overlap outside a tile and abut inside it are one run in the tile and two on the page, and the seam conflates on the page only. Measured 2 October 2026 on three one-sample images over a 100×100 page (the first and third abutting at `x = 40.5`, the third overlapping the second below the tile): the tile over the top half differs from the page on **40 pixels, all of column 40, by 63 levels at 1×**, pinned exactly by `an_image_run_that_overlaps_only_outside_a_tile_is_ruling_5s_named_exception`. This row was deleted on 26 September with the image-run case named in ruling 5 as having no fixture; a review built one, so the row is back, narrowed to the case. Holding the run's coverage over the page frame rather than the canvas reaches it, at the cost of every tile walking every image of a run over the whole page — the trade this row has to make rather than a fix slip in | whether a draw joins a run is decided in the page's frame without a tile paying the page's image work, and that pin becomes a byte-equality assertion | M |
| The document's own ink on a CMYK page | `RenderOptions::allow_cmyk` hands back the page composited over ink, but every colour is flattened to sRGB where its resource is read, so the ink is light converted back with maximum undercolour removal: a rich black `1 1 1 1 k` arrives as pure `K`, pinned by name in `a_page_asked_for_in_ink_with_the_opt_in_comes_back_in_ink` | a DeviceCMYK fill's components reach a `CmykA8` page unchanged, rich black included, with that pin flipped | M |
| PostScript and PCL output | none | decision: print pipelines are the only consumer | decision |

#### The G4 exit criterion changed, and the change is the point

This row asked for "a G4 encoder held to **the T.4/T.6 coder the TIFF tests
already carry**". That criterion cannot be met and should never have been
written: `tiff/tests.rs`'s coder is written in this repository, its own header
says there is no `.tif` in the tree and none is fetched, and holding an encoder
this project wrote to a coder this project wrote is the failure
[rulings.md](rulings.md) 13 and the self-round-trip rule exist to name. The
coder was still the right thing to promote *from* — it is where the code came
from — but not the right thing to be judged *by*.

What it was held to instead, both third-party:

- **ITU-T T.4 Tables 2, 3a, 3b and 4** — fetched 15 September 2026, read twice
  (text layer, then rendered pages at 170 dpi), all 195 run-length entries
  asserted in `ccitt::tests::the_run_tables_are_itu_t_t_4_s_own`. This is the
  only thing in the tree that reaches a make-up code, because no fixture has a
  run longer than 63.
- **ITU-T T.88 Annex H.1 segment 4** — 26 bytes of MMR, which 6.2.6 says *is*
  T.6, coding a bitmap the same annex publishes as a picture. Both halves are
  the standard's, so re-encoding the picture and comparing with the bytes is
  adjudicated end to end by a body that is not this repository.

The JBIG2 criterion was met as written, and its adjudicator is the same annex's
segment 11 — nine published bytes for the same picture at template 0. One limit
found in the doing and recorded at the test: **no published bitstream can pin
the context numbering**, because a context index is only a label into an array
whose slots all start identical, so a coder's output is blind to any bijection
of it. The numbering is pinned by T.88's Figures 8 to 11 instead.

### Text

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| Search by regular expression | `TextPage::search_with(needle, &SearchOptions)` has landed for the other three options — case-sensitive, whole word on UAX #29 boundaries, diacritic-insensitive by canonical decomposition and `Diacritic` marks removed — and `search` is unchanged; a pattern language is not among them, since the tree has no regex engine and links none | a decision: a hand-rolled engine (rule 1 admits no other), and if so which syntax and which guarantee against catastrophic backtracking on a hostile pattern | decision |
| Inferred reading order for untagged pages — columns, running heads, footnotes — labelled as inferred | content-stream order, joined geometrically — `TextDevice` sorts nothing, and the row used to say "geometric line and block order"; inference is a named refusal so a guess is never mistaken for the file's own order | an opt-in `ReadingOrder::Inferred` that is never the default, held to the 1 078 tagged corpus files (`corpus/ratchet.json`'s `tagged.files` summed over five corpora; the 717 this row carried was the sum over four) by inferring with the tree hidden and scoring against it, with the 589 one-paragraph veraPDF fixtures as the set where nothing may move rather than as the population | L ([design/reading-order.md](design/reading-order.md)) |
| Table reconstruction from geometry | none; the corpus carries 207 files with a `/Table` element, 1 908 tables, 145 of the files and 1 836 of the tables in SafeDocs, measured 16 September 2026 | opt-in, same discipline, held to the `/Table` elements the corpus carries | L ([design/table-reconstruction.md](design/table-reconstruction.md)) |

### Images and fonts

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| A CJK fallback face | none bundled or fetched; the 202 predefined CMaps extract CJK text, and nothing draws it without a host face | an OFL face behind `bundled-fonts` — `deny.toml` already admits OFL-1.1 — kept out of the wasm default so the 2.5 MB gate holds | M |
| Hinting | outlines are unhinted by design | decision: an autohinter is L and a fidelity question ruling 13 cannot adjudicate; stem darkening is S and measurable as stem width at small pixel sizes; revisit with a corpus of small-text scans | decision |

### Document operations

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| ~~Image recompression and downsampling on rewrite~~ **Landed for images stored through the general filters; images stored through an image codec are left whole by name** | **`tinker_pdf::write::SaveOptions::images`, off by default** (`ImagePolicy::Keep` never enters the pass, and `the_default_save_leaves_every_image_as_stored` holds a default save byte-identical to the font pass and the serializer alone). `ImagePolicy::Recode` names a coding per kind — continuous: deflate or baseline JPEG with **the caller's tables**; bilevel: deflate, T.6 G4 or a T.88 generic region in D.3's embedded organisation — and an integer box filter to a caller's `max_ppi`, the factor per axis `ceil(finest placement ppi / max_ppi)` over every page, form at any depth and annotation appearance that draws the image. `Saved::images` names every image by object reference, recoded or left whole with an `UntouchedImageReason`. Held by `crates/tinker-pdf/tests/image_recode.rs`: lossless codings give back exactly the samples written, and the page draws the same pixels; JPEG decodes within a bound computed in the test from the caller's table (T.81 A.3.3's basis on half of each quantiser, widened by a 1/16384 basis and a flooring integer IDCT, and for RGB T.871's inverse); downsampled dimensions and every block mean match a box filter written in the test. `tpdf`'s writing commands take it as `--images keep|flate|jpeg` (JPEG with the caller's `--jpeg-tables`), `--bilevel keep|flate|g4|jbig2` and `--max-ppi`, and print the report (`the_image_policy_recodes_and_resamples_when_asked_and_keeps_otherwise`). **Why `SaveOptions` and not `WriteOptions::images`, which this row first asked for:** a resolution is where an image is *drawn*, and only the interpreter's walk — which `tinker-pdf-cos` is below — can say where, so a `WriteOptions` field would be a flag the crate carrying it cannot act on (font subsetting's argument, `write.rs`); and `WriteOptions`' writer keeps a contract that it never re-encodes image bytes, which this pass, running on the editor before it, leaves intact. A `/Decode` array never blocks the pass: lossless codings keep the samples, and 8.9.5.2's affine map commutes with a block mean and only scales a quantiser's error | **What is left, by name.** (1) An image stored as `DCTDecode`, `JPXDecode`, `CCITTFaxDecode` or `JBIG2Decode` is `UntouchedImageReason::Filter` — its samples *in their own colour space* are the images row's read-side type, and recoding them waits on that rather than on a second decoder here. A photograph is commonly stored as DCT, so this is the part a downsampling caller is likeliest to meet. (2) An image only a tiling pattern's cell, a Type 3 glyph or a soft mask's group draws is recoded but never resampled (`Unplaced`): the walk interprets pages, forms and appearances only. (3) `/UserUnit` (PDF 1.6) is not read; placements are in 1/72 inch. No corpus measurement was taken: the fetched corpora are not in this container | S for (1) once the read-side type exists; S each for (2) and (3) |
| Encryption on save below R6 — R4 with AES-128, RC4 — for readers that stop at 1.6 | R6 only | decision: the readers that need it are the whole reason | decision |

### Annotations and forms

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| Per-subtype annotation payloads | **the payloads landed; the corpus count per family is owed.** `Annotation::payload` is an `AnnotationPayload`, one variant per 12.5.6 family (23, and `None` for an untyped entry), each carrying its table's entries with its table's defaults — `/QuadPoints`, `/InkList`, `/Vertices`, `/L` with `/LE`, `/LL`, `/LLE`, `/LLO`, `/DA`, `/IC`, `/CL`, `/BE`, `/RD`, `/FS` as a file specification, `/Open`, `/State`/`/StateModel`, `/Name` icons, `/Sound`, `/Movie`, `/3DD` and rich media by reference — beside Table 164's and 170's common entries, now including `/C`, `/BS`/`/Border`, `/CA`, `/RC`, `/Subj`, `/CreationDate`, `/IRT`, `/RT`, `/IT`, `/NM` and `/AS`; `carries_required()` says whether a family's required entries are there. Every family is held to a hand-written fixture in `crates/tinker-pdf/src/annotations.rs`. **The cap names what it dropped without mutating anything**: `Page::annotation_list()` returns an `AnnotationList` whose `dropped` counts the `/Annots` entries past 4 096, and a second bound, `MAX_ANNOTATION_BYTES` (64 MiB of copies a page, a `bounds_ledger.rs` row), closes an amplification the listing had from the start — one shared object named by 4 096 entries was copied 4 096 times — and is counted the same way in `incomplete`. What is left is the measurement: `crates/tinker-pdf/tests/annotation_census.rs` counts payloads per family and runs nightly in `corpus.yml` from 26 September 2026, but the fetched corpora were not reachable where it was written, so **no per-family count has been taken** and the census holds no floor on one. The census before this row: 14 996 annotations on 982 files, 14 986 covered, 10 refused across 6 subtype names | the per-family counts from the census's first nightly run, recorded here and held as floors | S |
| ECMAScript for forms beyond the subset | a deliberate subset under `ScriptPolicy` | decision: a full engine is XL and [design/form-script-policy.md](design/form-script-policy.md) argues against running document code by default; grow the subset by the corpus count of refused constructs instead | decision |

### Standards

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| PDF/UA validation | a measured abstention: 29 of 239 non-conforming fixtures caught, 210 abstained, 0 false alarms over 195 conforming, re-measured 16 September 2026; 24 of the 25 font-clause fixtures abstain, and about half of them ask for rules the PDF/A font group already has and does not run for a PDF/UA claim | a rule group that decides the decidable clauses and abstains by name on the rest | L, schedulable as an M half first ([design/pdfua.md](design/pdfua.md)) |
| PDF/X validation and writing | a `GTS_PDFX` intent is tolerated and never checked; **no annotated PDF/X conformance corpus exists** — the veraPDF corpus carries none, 23 fetched files claim a PDF/X flavour, and the two published suites (Ghent Output Suite 5.0, Altona 1.2) are conforming files only, so a validator would have a false-positive bar and no false-negative bar | an ISO 15930 rule group for the 2003 levels; the archival profile grows a PDF/X flavour; X-4 and X-6 wait on the standards' text | L for the 2003 levels; X-4 and X-6 unpriced ([design/pdfx.md](design/pdfx.md)) |
| PDF/E | absent | decision | decision |
| PDF 2.0: associated files, page-level output intents, namespaced structure types | encryption and the UTF-8 string type (read, and written for a document declaring 2.0) are the 2.0 deltas implemented | each read and written, and its row in [pdf20-deltas.md](pdf20-deltas.md) moved | S to M |

### Formats

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| ~~A standalone SVG, a bare image, a loose HTML or XHTML file, each as a document~~ **Loose HTML that is not XML** | **The SVG, the image and the XHTML file open.** `Document::open` sniffs them after the containers and before the PDF parser, a PDF header wherever the COS parser looks for one (its first 4 096 bytes) always winning (`tinker_pdf::standalone`): an SVG is a book of one pre-paginated chapter, an XHTML file one of a reflowable chapter, a bare image the comic of its one picture, paged by the comic path's own body (a JPEG, PNG, TIFF, JPEG 2000, GIF or WebP drawn, a multi-page TIFF one page per directory, an AVIF a placeholder naming its format). `tests/standalone.rs` holds a loose XHTML file pixel for pixel to the same bytes as an EPUB's one chapter, and a bare PNG, JPEG, TIFF, GIF, WebP and three-page TIFF to the same pages, pixels and warnings as a one-entry CBZ. **What is left is HTML that does not parse as XML**: it is read as far as it parses and `ArchiveWarning::Markup(Truncated)` says it stopped, because this build has no HTML5 tokenizer or tree builder — a document that leaves `<p>` or `<br>` unclosed, as most hand-written and generated HTML does, opens with what came before | an HTML5 tokenizer and tree builder (WHATWG §13.2.5 and §13.2.6) held to a **counted subset of html5lib-tests**' `tokenizer/*.test` and `tree-construction/*.dat` (MIT, on GitHub), vendored with provenance, and tag soup opening as the tree those tests describe | M |
| ~~Markdown; FB2~~ **An FB2 in an 8-bit encoding** | **Both landed.** `tinker_pdf::markdown` is a hand-written CommonMark 0.31.2 reader, opened by `Document::open_markdown` and made by `DocumentBuilder::from_markdown`, translated into an XHTML document and laid out by the EPUB path as a book of one chapter; held to the specification's own 652 examples by `tests/commonmark_spec.rs` over the fetched, never committed `spec.txt` (CC-BY-SA 4.0): **651 of 652 pass exactly**, measured 2 October 2026, every section whole but the entity one, whose example 25 names HTML5-only entities outside the XHTML 1.0 sets this repository vendors. `tinker_pdf::fb2` translates FictionBook 2.1 the same way: `Document::open` sniffs a `FictionBook` root, and the `.fb2.zip` it ships as, and `tests/fb2.rs` holds an FB2 pixel for pixel to the XHTML it translates to, its pictures to its own `<binary>` elements and a note link to its note. **What is left is FB2 in `windows-1251`, `koi8-r` and the other 8-bit encodings** a great many real files declare: `tinker-pdf-xml` decodes UTF-8 and UTF-16 only, so such a book opens as an empty page with `Markup(Truncated)` | an 8-bit decoder in `tinker-pdf-xml` from a vendored mapping table (the WHATWG Encoding Standard's `index-windows-1251.txt` and its siblings, CC BY 4.0, on GitHub), and the FB2 corpus a reader is for, which this repository has none of | S |
| MOBI; DOCX | absent | decision: a DOCX layout engine is a word processor | decision |
| Writing EPUB, XPS and CBZ | a decision item already | unchanged | decision |

### Surface

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| ~~A user-facing CLI~~ **Landed, without `sign`; one limit found in the facade, and two refusals the facade owes** | eighteen subcommands. The ten reading ones, `fonts` and `images` among them, and eight that write — `merge`, `split`, `rotate`, `encrypt`, `decrypt`, `attach`, `stamp`, `sanitise` — each a wrapper over the facade (ruling 11): `import_page`, `keep_pages`, `rotate_page`, the encryption on `WriteOptions`, `attach_file`, `import_page_as_form` with `stamp`, and `sanitise`, each flag one `Sanitise` field and `--all` `Sanitise::ALL`. Three decisions the CLI first made on its own the facade now makes for every surface: a turn that is not a quarter (`rotate_page` refuses it, where it rounded), an empty owner password (the user's, as Algorithm 3 has it for R2 to R4, where it handed the owner's authority to anyone trying the empty password), and `sanitise` with no flag (refused; it meant all four, which `Sanitise::default()` does not). A writer refuses a flag it would ignore, `--password` on an input that is not encrypted, and a page its list names twice. **Every one of them saves through `tinker_pdf::write::save` and takes `--font-policy subset|keep`**, with the facade's default, subset, and prints the pass's report, a program left whole named with its reason — the surviving half of the closed "Font subsetting on rewrite" row, so `docs/features/editing.md`'s refusal for it is gone. Every writer rewrites, never appends, so what a split, a sanitise or an encrypt took out is not left in a prefix; `split` garbage-collects and `merge` deduplicates, the option each editor call's own documentation pairs it with. **Two refusals are the CLI's own, so the CLI and the C ABI or bindings still answer one request differently**: an encrypted input is refused by every writer but `encrypt` and `decrypt`, since the facade rewrites it decrypted when no encryption is asked for, and those two want the owner's authority where the user is restricted, since the facade reports permissions and does not enforce them. Held by `tools/tpdf/src/writing.rs`: each command run on `testdata/` fixtures and built documents, its output reopened through the facade and held to the strict validator. **`sign` is left out, and not as a gap**: signing is a private-key operation and keys in the engine are a non-goal of [design/signatures.md](design/signatures.md) — the `Signer` seam takes finished CMS bytes, which a command line cannot produce without the signing arithmetic that non-goal declines. **The limit, the facade's rather than the CLI's**: `keep_pages` keeps the catalog, and a garbage-collecting rewrite follows it, so a page an outline item, a named destination or a link still names stays in a split piece outside its page tree — page 6 of `outline-3level.pdf` split out carries five page objects (`a_page_the_outline_names_stays_in_a_piece_that_dropped_it`, editing.md's refusal table). `tpdf split` says it is not a redaction | `sign` stays out while signing keys are a non-goal. **Decision, the owner's**: whether the facade refuses a rewrite of an encrypted document that asks for no encryption, and a change of encryption made with only the user's authority over withheld permissions — `write::save` returns bytes, so either refusal is an API change — after which the CLI's two refusals become the facade's and ruling 11 holds without exception. A page dropped by `keep_pages` or `delete_page` no longer reachable from anything the saved document keeps: references to it nulled or pruned on save, the pinned test inverted | S to M, in `tinker-pdf-cos` |
| Java, Swift, Go and Ruby bindings | none | each over the C ABI in the .NET pattern, with the smoke and parity scripts; which first is a decision | M each |

## Named non-goals

Decisions, kept in one place so none is mistaken for an omission. Two kinds,
and the difference matters under the goal this file now states.

**Hard limits, for a reason outside this repository's power.**

- **XFA** — removed in ISO 32000-2 ([features/forms.md](features/forms.md)).
- **RAR's compression**, which has no published specification and one
  decoder whose licence bars derivation; and **RAR 4** entirely, which no
  producer here can write, so a decoder would be unadjudicated under ruling
  13 ([features/cbz.md](features/cbz.md),
  [design/comic-archives.md](design/comic-archives.md)).
- **A bundled sRGB profile** — the ICC's own carry no SPDX identifier, and
  which device an archival document's colours are for is the caller's
  statement ([design/pdfa.md](design/pdfa.md)).
- **The legacy Mac OS encoding tables**, and therefore a Macintosh `cmap`
  subtable read in a non-Roman encoding. Apple's published mapping files
  disclaim warranty and grant no redistribution rights, so they have no SPDX
  identifier `deny.toml` allows and `cargo xtask vendor` refuses them — the
  same limit as the sRGB profile above, reached from the other direction. It
  costs nothing measurable: 28 of the corpus's 7 905 embedded faces carry such
  a subtable and **every one of them carries a Unicode subtable beside it**,
  which `glyph_for_char` prefers. What the engine owes and now does is refuse
  rather than mis-map — above U+007F a Macintosh subtable returns nothing, so
  a caller can fall back instead of drawing the wrong glyph
  ([features/fonts.md](features/fonts.md)).
- **Font directories** — `wasm32-unknown-unknown` has none, so `local()` is
  the host's to answer through `FontProvider`
  ([features/epub.md](features/epub.md)).
- **The four properties that left with the oracles** — a reader nobody here
  wrote accepting this engine's output, a second reading of an XPS package,
  a reference CSS implementation, an arbiter for an EPUB disagreement. Ruling
  13 retires them and [verification.md](verification.md) names them.
- **JPEG 2000 component precision above 16 bits.** T.800 Table A.11 allows
  1 to 38 — `Ssiz` runs `x000 0000` to `x010 0101`, "component sample bit
  depth = value + 1" — and this build refuses past 16. **The coefficient
  plane is what caps it, not the output sample.** E.1's dequantisation clamps
  a coefficient to `2^(R_b + 2)` sample units, and a plane entry is Q12 in an
  `i32`, so a coefficient on the clamp occupies `2^(R + 14)` of the plane's
  own format: `2^30` at 16 bits, which is `PLANE_BOUND` exactly, and `2^31` at
  17, which an `i32` does not hold. That is not the whole reason and it would
  be revisitable on its own — an `i64` plane would hold it, at twice the 268 MB
  a 4096 x 4096 four-component tile already costs, and at the price of
  re-proving the `MAX_PRODUCT` bound the fixed-point 9/7 rests on. **What
  makes it a limit is that there is nowhere to hand the result**: ISO 32000-1
  Table 89 gives `/BitsPerComponent` as 1, 2, 4, 8 or 16 and nothing wider,
  so a widened sample would be narrowed again one stage later, where the
  narrowing is less visible rather than absent.

  **Not the same bargain as JPEG's twelve-bit frames**, which decode and are
  narrowed to eight on the way out with `JpegPrecisionNarrowed` recorded. That
  works because a JPEG coefficient path is fixed at the frame's own precision
  and only the *sample* needed narrowing; a JPEG 2000 coefficient's magnitude
  grows with `R_b` by E.1, so there is no stage at which the wide value does
  not have to exist. The comparison is drawn because it is the obvious
  objection and it was checked rather than assumed.

  T.800 says the same thing twice from its own side. Table A.11's footnote a)
  warns that "not all combinations of coding styles will allow the coding of
  38-bit samples", and **every profile T.800 names caps the depth at or below
  this**: Table A.45's Profiles 0 and 1 at `7 <= Ssiz_i <= 11` (8-12 bits),
  Tables A.48, A.51 and A.52's Broadcast Contribution and IMF profiles at
  `7 <= Ssiz_i <= 15` (8-16 bits). Zero of the corpus's 39 JPX files reach it.
  `the_coefficient_plane_is_what_caps_precision` in
  `crates/tinker-pdf-filters/src/jpx/tests/bounds.rs` asserts the relation
  rather than the figure, so this entry fails a test if its premise stops
  being true ([features/filters.md](features/filters.md)).
- **JPEG XR's unadjudicated list** — the quantised lossy path past QP 1, the
  first-level overlap filter across a soft tile boundary, `HARD_TILING_FLAG`,
  `SHIFT_BITS`, `TRIM_FLEXBITS`, more than one QP per tile, and a damaged
  codestream's surviving values. All of it **decodes**; nothing here checks
  that the result is *right*, and no amount of testing shrinks the list.
  Ruling 13 bars a second decoder from adjudicating an output, so the only
  thing that could close it is T.832's own conformance bitstreams, and those
  are not freely licensed. It can shrink if that changes and by nothing this
  repository can do meanwhile, which is why it is a limit and not a row.
  [features/xps.md](features/xps.md) and
  [features/filters.md](features/filters.md) carry the list;
  [design/jpeg-xr.md](design/jpeg-xr.md) carries the reasoning and the three
  first-party evidence legs that stand in an oracle's place. **Not the same
  as JPEG XR encoding below**, which is a scope choice under ruling 8 and
  could be revisited on evidence: this one could not.

**Decisions this repository took and keeps, each revisitable by a row above
if the field's evidence asks.**

- **Shaping while reading a PDF** — `TJ` arrays are honoured as written
  ([features/fonts.md](features/fonts.md)).
- **Encryption inside archives** — ZipCrypto, AES in ZIP, 7z and RAR — and
  **multi-volume or sparse entries**
  ([design/comic-archives.md](design/comic-archives.md)).
- **EPUB scripting, `META-INF/signatures.xml`, real resource encryption**
  (this engine holds no key) ([features/epub.md](features/epub.md)).
- **Signing keys in the engine, revocation fetching, a bundled root store**
  ([design/signatures.md](design/signatures.md)); and **no callback into
  host code across the C ABI**, which is why `save_signed` and `Signer` are
  not projected ([design/bindings-write.md](design/bindings-write.md)).
- **Recipient shapes other than key transport** in public-key envelopes
  ([design/pubsec.md](design/pubsec.md)).
- **JPEG XR encoding**, and applying the container's orientation transform,
  which T.832 leaves to an application this crate is not (ruling 8)
  ([design/jpeg-xr.md](design/jpeg-xr.md)).
- **JPEG 2000 Part 2** (ISO/IEC 15444-2) — a marker Table A.2 does not
  define is refused as unknown rather than measured past
  ([features/filters.md](features/filters.md)).
- **SVG `<switch>` conditional processing** ([design/svg.md](design/svg.md)).
- **SVG's `<filter>`, `<foreignObject>`, SMIL and `<script>`**, each drawn without
  and named. A filter is a raster pipeline over a rendered subregion, and PDF has
  no filter: drawing one means rasterising inside the writer, at a resolution the
  book does not have, and losing the vector text under it. A `<foreignObject>` is
  another document language, which this leaf crate must not lay out (ruling 8).
  An animation's static rendering is its state at time zero, which is what paper
  can show; a script is never run, as in the rest of the book
  ([design/svg.md](design/svg.md)). `<textPath>`, `<tref>` and `<altGlyph>` are
  not here: having no file behind them is a scheduling fact, and they wait in the
  SVG row of tier 4 for a count.

Six things the docs called non-goals became rows in tier 5, because the
field offers each and the reason given was a scope choice rather than a
limit: hinting, a visible signature appearance, timestamp creation, writing
a public-key-encrypted document, image encoders, and inferred reading order
for untagged pages. Each row says so — and the visible signature
appearance's has since closed (`SigningTarget::NewVisibleField`,
[features/signatures.md](features/signatures.md)), and so has timestamp
creation's (`DocumentEditor::save_timestamped` with a host `Timestamper`,
held to real RFC 3161 tokens over this engine's own output), and so has
writing a public-key-encrypted document's (`PublicKeyEncryption::seal` and
`DocumentEditor::save_sealed`, the envelope held to RFC 5652's encoding as
OpenSSL's is, and opened by OpenSSL, [features/encryption.md](features/encryption.md)).

## How this file changes

An item leaves this file when its exit criterion is green, and its feature
doc's refusal table loses the matching row in the same commit. There is one
other way out and it is not a shortcut: an item whose exit criterion can
never go green is not work this file is tracking, it is a **limit**, and it
moves to Named non-goals with the reason it cannot close. An item enters
with evidence attached — a corpus number, a named refusal, an owed
assertion — or it does not enter. Design docs in [design/](design/) carry
scope, non-goals, design, milestones with concrete exit criteria, and
risks, in that order.
