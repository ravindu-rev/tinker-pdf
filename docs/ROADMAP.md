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

The suite stands at 5 028 passed, 0 failed, 60 ignored across 221 suites as
[verification.md](verification.md) records it, measured 21 September 2026 on
`x86_64-pc-windows-msvc`.

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
| Memory | 44 caps in `bounds_ledger.rs`, two of them the runtime bounds a *process* spends and one of them a relation between a symbol and its page rather than a quantity; every corpus child measures its own peak resident set, `report.json` carries it per file and `ratchet.json` bands the per-corpus maximum within a **measured 2 % tolerance** — a high-water mark swings 0.06 % to 0.76 % between two runs of one binary, and an exact band failed on that within a day of being recorded | ratcheted, over five corpora |
| Fidelity | arithmetic fixtures, metamorphic relations, committed fingerprints | tier 1's differential pairs and reviewed goldens; ruling 13's amendment of 5 September 2026 on dated outside measurements |
| Capability coverage | tiers 2 to 5 of this file | each row's exit criterion |
| Footprint | 2.03 MB of wasm, 1.40 MB gzipped, gated at 2.5 MB in `release.yml` | already ratcheted |
| Surface | 123 C functions; four bindings, none projecting the whole facade; nine CLI subcommands, every one read-only | tier 3's bindings row; tier 5's CLI and bindings rows |
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
| **Signatures.** RSASSA-PSS, named and not decoded; `adbe.pkcs7.sha1`, one corpus file and it is a fuzzer's output; a signature with no signed attributes; RFC 3161 timestamps, parsed and not validated; `GeneralNames`, carried and not decoded | [features/signatures.md](features/signatures.md); [design/signatures.md](design/signatures.md) | each wired against a published vector or a real signature | S each; timestamps M |
| **Public-key encryption.** **The content-cipher gap is closed**: `des-ede3-cbc` decrypts — the tables read twice from FIPS 46-3 and adjudicated by 500 NIST CAVP known answers — so the cipher OpenSSL still picks by default for older recipients now opens rather than being named. AES-192-CBC and RC2, which `cms -encrypt` can also be asked for and which no PDF has ever defaulted to, remain refused by name. What is left is the half the row always said was not work: the 7.6.5 key derivation is held only to a second implementation by the same author, and that risk closes the day a real public-key-encrypted document arrives. Re-measured 15 September 2026, and the clause survives — `/Adobe.PubSec` and `/Recipients` are each **zero across all 5 605 PDFs** under `corpus/files`; ten files match `pubsec` case-insensitively and every one of the ten is `/PubSec` inside a signature's `/Prop_Build`, a different key entirely | [features/encryption.md](features/encryption.md); [design/pubsec.md](design/pubsec.md), Risks; `des.rs` in `tinker-pdf-crypto` | a real public-key-encrypted document in a corpus | the risk is not work |
| **Non-device colour spaces on write: CIE, `/Separation` and `/DeviceN`.** **The ICC half has landed** — `set_fill_icc` and `set_stroke_icc` write 8.6.5.5's `/Name cs c1 … cn scn` with the operand count taken from the space's own `/N` rather than from the caller, and `ImageColorSpace::Icc` lets an image name a registered space (8.9.5.4). What is left has no partial: a `/Separation` or `/DeviceN` space is a tint transform into an alternate space, which is a function this writer would have to emit and nothing here emits one yet | [features/creation.md](features/creation.md); `add_icc_color_space` and the two setters in `crates/tinker-pdf-cos/src/build.rs` | the fill and stroke setters take a `/Separation` name and a tint transform, and `add_image` takes the same | M |
| **Editing.** Redaction cuts a rotated or skewed run along its own baseline; what it still refuses to *measure*, and therefore leaves whole and names in `RedactionReport::warnings`, is a **vertical writing mode** (9.4.4 advances by `/W2`'s `w1` down the page and a `TJ` number displaces along that axis, which is a different formula rather than a different matrix) and a **Type 3 font whose `/FontMatrix` is not the 1/1000 default** (9.6.5 puts its `/Widths` in a glyph space `width / 1000` does not map out of). The other two warning classes — a `Tf` naming a font the resources in scope do not have, and a text rendering matrix whose composition is not finite — are permanent and live in the feature doc's refusal table, not here, because neither has an exit: a run with no metrics has no positions to compute. Text inside a Type 3 glyph procedure or an annotation appearance stream is not rewritten; appearance synthesis covers seven annotation subtypes | [features/editing.md](features/editing.md); the four unmeasurable-run variants of `RedactionWarning`, each with a test of its own in `crates/tinker-pdf/src/redact.rs`'s `refusals` module — the type's fifth variant, `RepeatedForm`, is about a form rather than a run and belongs to the row below | a vertical run measured by `/W2` and cut; a Type 3 font's glyph space read from its `/FontMatrix` rather than assumed; `/AP` and glyph-procedure streams rewritten; an `/AP` per further subtype | S; S; M; S per subtype |
| **A form XObject drawn twice is cut to the union of its placements, not exactly at each.** *The under-redaction this row was opened for is closed:* the guard is keyed by the transform as well as by the object, so a form is rewritten once per distinct placement and a rectangle over the second placement is measured against it — `glyphs` is no longer 0 where content was covered, and the pin asserts the fix. What is left is exactness. A form is one stream however often it is drawn, so a glyph cut for one placement is gone at all of them, including placements no rectangle touched; that widened cut is named by `RedactionWarning::RepeatedForm` rather than silent, and over-removal is the direction this module errs in everywhere else too. The exact answer is a copy of the form per `Do`, rewritten in that placement's frame — **and what it waits on is not redaction.** `PageResources::xobject` resolves an `/XObject` name through the `CosDocument`, and `form_from` reads the stream the same way, so an object a `DocumentEditor` has only just allocated is invisible to it. `subset::apply` runs *after* a redaction and walks the page through `PageResources`, so a copy would not be entered, and glyphs surviving only in the copy would be cut from the subsetted font and drawn blank — against that section's own "err toward inclusion" rule. Making the copy work means an editor-aware resolution path in `PageResources`, which rendering, extraction and the PDF/A validator share; that is the cost, and it is why this closed the leak rather than the inexactness | `a_form_drawn_twice_is_cut_at_the_placement_the_rectangle_covers` and five more in `crates/tinker-pdf/src/redact.rs`, the seven-defect injection campaign in its module header (20 September 2026, 1 593 tests, no zeros); [features/editing.md](features/editing.md) | a copy of the form per distinct placement, so `RepeatedForm` stops being raised for a form the rectangles did cut; `subset`'s walk entering the copies, asserted by a fixture whose two placements are cut differently | M, and the `PageResources` half of it is shared with every other editor-then-walk pass |
| **Bindings.** Owed projections: the rest of `PageBuilder` (`encoded_text`, `glyphs`, `form`, `shading`, the two pattern setters, `set_ext_gstate`, `set_bleed_box`) and of `DocumentBuilder` (`add_named_font`, `add_cid_font`, `glyph_run`, `add_ext_gstate`, `add_form`, `add_shading`, `add_tiling_pattern`, `clear_image_resources`); `DocumentEditor`'s `import_page`, `keep_pages`, `flatten_annotations`, `add_annotation`, `reset_form`, `set_field_values`, `set_calculated_values`; `PageBuilder::tagged` as an `open_tag`/`close_tag` pair; a signature's `/Contents`, `/M`, `/ContactInfo`, `/Filter`, its warnings and `modifications`; signatures in Python and JavaScript; the read surface — outline, links, metadata, attachments, XMP, warnings; `authenticate_with_recipient` | [features/bindings.md](features/bindings.md); the C ABI exports 123 functions today | each through the C ABI and the three wrappers with a parity-script line; `cargo xtask bindings-parity` green | L ([design/bindings-write.md](design/bindings-write.md) covers the write half; the read half is owed a section there) |
| Published packages: `pip install`, `npm install` and `dotnet add package` do not work, by decision, until the facade freezes at 0.1.0 | [features/bindings.md](features/bindings.md); the release pipeline has run once and published nothing | a `workflow_dispatch` with `publish: true` | a decision, then S |
| PDF 2.0 deltas still "tracked": the UTF-8 string type; the 2.0 blend and dash clarifications | [pdf20-deltas.md](pdf20-deltas.md) | each row moves to a status with a doc behind it | S each |
| XML: three allowed public identifiers, no HTML named-entity table, and a `DOCTYPE` with an internal subset is refused | `crates/tinker-pdf-xml/src/lib.rs`; the exit criterion is already written in its tests | the entity table vendored from the published list, with provenance | S |

**Decision items, not commitments.** Each is a choice recorded so it is not an
omission, and each waits on evidence rather than on time: **OCR**, if ever, as
a host seam like `FontProvider` and not an engine; **container writing** — CBZ,
XPS and EPUB are read-only conversions; **an archival profile on
`WriteOptions`** — `rewrite` returns `Vec<u8>` with nowhere to put a refusal,
which is why the builder has one and the rewriter does not
([design/pdfa.md](design/pdfa.md)); **image encoders the *writer* reaches for**
— `tinker-pdf-filters` writes PNG, CCITT G4 and JBIG2 generic regions as of
15 September 2026 and baseline JPEG as of 16 September, and the document writer
calls none of them, so partial image
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
| **EPUB CSS: seventy properties known and unimplemented**, each reported as `UnimplementedProperty` with an element count, against the 100 names `IMPLEMENTED_NAMES` carries, longhands and shorthands alike. The ones that change a paged output: `transform`, `opacity`, `overflow`, `box-shadow`, `text-shadow`, `border-radius`, the `background-*` image family, `list-style-*`, `counter-reset` and `counter-increment`, `quotes`, `text-transform`, `writing-mode`, `direction`, `unicode-bidi`, `hyphens`, `break-*`, `page`, `outline`, `clip-path`, `filter`, `mix-blend-mode`, `grid*`, `font-feature-settings`, `font-kerning`; and custom properties, which none of the committed books declares | `UNSUPPORTED_PROPERTIES` in `crates/tinker-pdf-css/src/property.rs`, counted; the census there measured 84 distinct names across the fetched corpus's 53 stylesheets and 42 across the committed 8 | scheduled by the fetched corpus's `UnimplementedProperty` counts, highest first; each landing deletes its name from the table | L — [design/epub-layout.md](design/epub-layout.md), written with the replaced box and carrying the slices as its milestones 2 to 7 |
| EPUB layout refusals, each a typed warning in `tinker-pdf-layout`: `column-span: all` laid out as `none`; `max-height` shorter than its content treated as `auto`; an atomic box taller than a page inside a table band, a flex line or a column, drawn where it starts; `inline-flex` as a block-level container; a block inside an inline laid out as a block; a table column's background and border never painted; anonymous row generation; `::first-line` and `::first-letter` parsed with no box; `content: url()`, `counter()`, `counters()` and the quote keywords generating nothing, and `quotes` itself; `:nth-child(An+B of S)` dropped; `@page` and `@supports` skipped | [features/epub.md](features/epub.md) | each row's warning stops firing on a reftest pair in `epub_reftest.rs` | S–M each |
| EPUB text set in a standard-14 fallback face loses characters past a simple font's 256 codes — 224 outside `WinAnsiEncoding` — counted as `UnrepresentedCharacters`. **Verified: an embedded face is exempt.** `epub/paint.rs` returns on `Chosen::Embedded` before any counting, and only `Chosen::Standard` reaches the counter, so this is the standard-14 fallback alone | `crates/tinker-pdf/src/epub.rs` pushes the warning | a CID-keyed path for fallback text; the conservation harness unchanged | M |
| EPUB features tallied and not implemented: MathML layout (one fetched book has 71 `mathml` items), media overlays, `switch`, `remote-resources`; and a font provider attached after open cannot re-paginate, since advances decided the line breaks at open | [features/epub.md](features/epub.md) | MathML and overlays are decisions to take against corpus counts; re-pagination is a design question before it is work | decisions |
| EPUB corpus gaps: no committed stylesheet declares a cascade layer, so `@layer` is verified against this engine's own reading of css-cascade-5 and against no producer; no producer book carries a `.woff` or `.woff2` in its ZIP, so the `@font-face` path is held to containers this repository packs | [features/epub.md](features/epub.md); nine committed books | a real producer's book for each | S, fixture hunting |
| **SVG in the spine**, refused by name with a typed warning each: `<filter>`, `<mask>`, `<pattern>` as a paint, `<marker>` (32 in the fetched corpus, all on paths that also fill), `<foreignObject>`, SMIL, `<textPath>`, `<tref>` and `<altGlyph>`, `spreadMethod` `reflect` and `repeat` drawn as `pad`, a per-glyph `x`/`y`/`dx`/`dy`/`rotate` list past its first value, a clip path whose children are `<use>` or `<text>`, at-rules inside `<style>`; and group `opacity` flattened into each descendant's alpha, too dark where a fill and a stroke overlap | `crates/tinker-pdf-svg/src/lib.rs`; [design/svg.md](design/svg.md) | markers first, being the one with a count; group opacity as a real transparency group | S for markers, spread and glyph lists; M for group opacity and masks |
| **XPS.** A `ContextColor` in a gradient stop approximated; a profile with a channel count `/ICCBased` cannot state (`nCLR` past 1, 3 or 4) painted grey, for want of a tint transform from profile evaluation; `StyleSimulations` drawn unsimulated; per-stop alpha and a `ColorInterpolationMode` this build does not interpolate in, approximated; interleaved OPC packages refused, with no corpus package using one | [features/xps.md](features/xps.md) | each `XpsElementDefect` row leaves the doc with a conservation-census fixture | S for simulations and OPC; M for stops and alpha; L for `nCLR`. **`{ColorConvertedBitmap}` has left this row**: the picture is drawn and its profile embedded as an `/ICCBased` space, and `gs-images.xps` joined the conservation sweep with it |
| **Archives.** A JPEG 2000 entry in a CBZ is a placeholder page although the JPX decoder exists; ZIP method 14 is refused although the LZMA decoder exists; GIF and BMP need a decoder each; WebP and AVIF need a codec each; bzip2 and Zstandard; 7z's PPMd coder, its **BCJ filter chain** (`03030103`, outside `sevenz.rs`'s coder allow-list, so `-mf=BCJ` is refused at open) and its BCJ2 folder, which takes four input streams | [features/cbz.md](features/cbz.md); every CBZ fixture is in the tree, so there is no corpus count. The BCJ fixture is deliberately **not** committed ahead of the decoder, unlike the three `.cb7`s: an archive a build refuses at open proves nothing about the coder it would run, and `tests/cbz/README.md` says so | wire the two decoders that exist first; the rest by evidence | S, S; M for GIF, BMP, bzip2, PPMd, BCJ, BCJ2; L for WebP, AVIF, Zstandard |
| TIFF, reached from XPS and CBZ: `PhotometricInterpretation` 4, 5, 8 and 32803, `Compression` 6 and 34712, `SampleFormat` 2 and 3, `Predictor` 3, BigTIFF, and directories after the first | [features/filters.md](features/filters.md); no count recorded | by evidence | S–M each, unscheduled |
| JPEG XR's named refusals: fixed-point, half-float and float pixel formats; the packed sub-byte depths; CMYK, CMYKDIRECT, NCOMPONENT and RGBE; an unknown pixel-format GUID; the interleaved alpha plane; YUV420, YUV422 and YUVK; a windowed origin | [features/filters.md](features/filters.md), each reached by `jxr/tests/refusals.rs`; the platform encoder cannot emit most of them | a fixture from a real encoder per row | unscheduled |
| Fonts on write: a CID-keyed CFF under `add_cid_font` is refused; Symbol and ZapfDingbats have no bundled equivalent and are unreadable when nothing embeds them; WOFF2 table transforms this build does not read are refused by name | [features/fonts.md](features/fonts.md) | by evidence | S each |

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
| ~~Image encoder: JPEG~~ **All three image encoders are written. Two named gaps are left and neither is a coder** | **The last of the three landed 16 September 2026.** `tinker_pdf_filters::jpeg_encode` writes ITU-T T.81's **baseline** process and only that: sequential DCT, Huffman, 8-bit, one SOF0 frame, one scan, `Ss = 0`, `Se = 63`, `Ah = Al = 0`. Grayscale, or T.871 clause 7's YCbCr with the JFIF APP0 that clause 10.1 requires so a reader is told which YCbCr it is; 4:4:4 or 4:2:0 **named by the caller and never inferred from the pixels**; Annex K's quantisation tables, those tables halved (K.1's own second paragraph is the only other setting T.81 names) or the caller's own, with **no 1-to-100 quality number** — every such scale in circulation is some program's private convention and none is in T.81, T.83 or T.871, so the caller who wants a specific quality supplies the table; A.2.4's partial MCUs completed by its NOTE's replication of the right-most column and bottom line; an optional restart interval. Progressive and extended sequential, arithmetic, lossless, hierarchical, 12-bit, four-component CMYK/YCCK and K.2's optimised tables are excluded **by name** in the module header, each with its reason. **The document writer calls none of the three** — `tinker-pdf-cos` imports no encoder, and the facade's own `png_encode` call inside `Bitmap::to_png` is the one place any of them runs. The writer still never re-encodes image bytes, which is a contract and not a missing coder. PNG stays the one image format the facade projects — `Bitmap::to_png`, `tpdf render` and `png_encode` | **The coder is done; two things are not, and the second is the row's own exit criterion failing.** (1) *A caller*: a writer that builds a `/DCTDecode`, `/CCITTFaxDecode` or `/JBIG2Decode` image XObject from a raster, which needs a decision about when a lossy coding beats deflate — a policy question, not a coder. (2) **No published DCT vector set adjudicates the coefficients this produces.** ITU-T T.83 (ISO/IEC 10918-2) is the compliance data published for exactly this purpose, and its own clause 4.4 says the data ships *on three diskettes* accompanying the document rather than inside it; the ITU's copy returned HTTP 500 on 15 and again on 16 September 2026 and the Recommendation's own page says it "is only available through payment", ISO's returns HTTP 403, and the one reachable copy is the standards-preview extract — read here with `tpdf`, it carries the numbered pages 1 to 11 and stops mid-sentence in clause 5.2.1, where its own contents list puts clause 6's encoder compliance tests on p. 19, Annex B's compliance quantisation tables on p. 28 and Annex C's compressed test data on p. 30. The Internet Archive holds nothing: no full-text hit, a 429 from the availability API and a 404 on a direct Wayback fetch. So the FDCT is held to **A.3.3's equation recomputed in `f64` in the test** — the standard's formula, not the standard's numbers — and the row says so rather than letting a round trip stand in. What *is* adjudicated by published data: Figure A.6's zig-zag read twice, Tables K.1 and K.2 read twice, K.3.3's four DHT byte lists appearing verbatim in the output, and two whole entropy-coded segments derived from Tables K.3's and K.5's printed code words with no implementation in the middle | closed for the coder; M for a caller; fixture hunting for T.83, which may never end |
| A PNG *read back* by `pdfcmp` | `tpdf render` writes `.png` and `pdfcmp` reads `.pnm` and `.pdf`, so its output no longer feeds the comparator. `xtask`'s `TOOLS` table keeps a tool to the facade, and the facade publishes an encoder and no decoder. **The decoder is not what is missing**: `tinker_pdf_filters::png_decode` is a public export and this crate's own tests already drive it, so what is owed is a facade entry point rather than a coder | either a facade entry point that turns PNG bytes into a `Bitmap`, or an argued exception in `TOOLS`; the decision belongs to whichever is written first | S |
| A tile byte-equal to the page at **every** scale | `RenderOptions::region` takes a `PixelRegion` and ruling 5's guard exists (`crates/tinker-pdf/tests/render_regions.rs`, September 2026): ten fixtures — text, a diagonal axial shading, an image and strokes, three of them turned and three cropped — tile byte-equal at 64, 37, 23 and 53 pixels against a 91×131 page, at 0.5×, 1×, 2× and 4×, down to a one-pixel lattice. **At 0.75×, 1.5× and 3× it is not unconditional**: 29 of 30 lattices are exact and the thirtieth, the shading at 3×, differs on one pixel of 107 289 by one level, because a page frame and a tile frame evaluate `a·x + c·y + e` at two magnitudes and round apart in the last ulp. `fill` and `draw_image` each quantise to the 1/256 grid and so are immune; a shading sampler goes from the affine to a colour with no grid in between | the canvas carries its origin through the shading sampler, the mesh and the image run so both frames compute in one lattice, and `at_the_scales_where_two_frames_round_apart_the_gap_is_one_level` becomes an unconditional byte-equality assertion | M |
| CMYK page output; a premultiplied-alpha option; an anti-aliasing switch | `CmykA8` is internal to transparency groups; alpha is straight only; no quality knob | each a `RenderOptions` field with its own fingerprint; the switch changes no determinism claim | S each |
| A form XObject or an annotation rendered on its own | pages only | `Page::render_form`, `Page::render_annotation` | S |
| A retained page — a display list replayed at any scale | every render re-interprets the content stream; `tinker-pdf-svg`'s `Scene` is the shape. **The recording `Device` this row asks for has landed** — `tinker-pdf-content`'s `record.rs`, promoted out of the interpreter's test module as a prerequisite rather than as a capability, so it has no row of its own; five other rows share it ([content-and-text](features/content-and-text.md)). Nothing replays it yet, which is all that is left of this row | a replay byte-equal to a direct render at every scale the fingerprints use | M |
| PDF to SVG | SVG is input only | a `Device` that writes SVG 1.1, held by reading its output back through `tinker-pdf-svg` | M |
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
| Word segmentation and word boxes | glyph, line and block quads; no word. `TextLine` has no `impl` at all. **The shaper's UCD carries no UAX #29 table** — its eleven vendored files are UAX #9, #24 and #15, and nothing in the tree reads `Word_Break`. The vendoring machinery to copy is one crate over: `tinker-pdf-layout` generates its UAX #14 line-break table from `data/ucd` in its own `build.rs` | `TextLine::words()` on UAX #29 boundaries, with `WordBreakProperty.txt` vendored and its table generated the way the line-breaker's is | M, and the size is the vendoring |
| Structured text serialisation — JSON, XML, HTML — with fonts, sizes and boxes | the model exists and nothing serialises it; no serde in the tree and none wanted | hand-written writers, so zero third-party logic crates stays true (no first-party manifest names serde; `Cargo.lock` carries it transitively through `criterion`, which is a dev-dependency of the benchmarks); `tpdf text --json`; the font name carried on the span | M |
| Search options: case-sensitive, whole word, diacritic-insensitive; regular expressions | literal and case-insensitive, one argument | an options struct for the first three; regular expressions are a decision, since the tree has no regex engine and links none | S; decision |
| Inferred reading order for untagged pages — columns, running heads, footnotes — labelled as inferred | content-stream order, joined geometrically — `TextDevice` sorts nothing, and the row used to say "geometric line and block order"; inference is a named refusal so a guess is never mistaken for the file's own order | an opt-in `ReadingOrder::Inferred` that is never the default, held to the 1 078 tagged corpus files (`corpus/ratchet.json`'s `tagged.files` summed over five corpora; the 717 this row carried was the sum over four) by inferring with the tree hidden and scoring against it, with the 589 one-paragraph veraPDF fixtures as the set where nothing may move rather than as the population | L ([design/reading-order.md](design/reading-order.md)) |
| Table reconstruction from geometry | none; the corpus carries 207 files with a `/Table` element, 1 908 tables, 145 of the files and 1 836 of the tables in SafeDocs, measured 16 September 2026 | opt-in, same discipline, held to the `/Table` elements the corpus carries | L ([design/table-reconstruction.md](design/table-reconstruction.md)) |
| Hyphen rejoining at line ends | never, unless the producer wrote `/ActualText` | opt-in on `plain_text`: a soft hyphen always, a hard hyphen at a line end followed by a lower-case start; counted | S |

### Images and fonts

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| Image extraction with decoded samples and colour space | raw streams through `Document::cos()` only. The decode path exists — `PageResources::image` yields a `DecodedImage` the renderer already uses — but it carries **no colour space**, being RGB by the time it is returned, and `ImageData` is the *write* side's enum rather than a read-side type | a read-side image type carrying decoded samples **and** the space they were in, since neither existing type is it; `Page::images()`; `tpdf images` | M |
| A CJK fallback face | none bundled or fetched; the 202 predefined CMaps extract CJK text, and nothing draws it without a host face | an OFL face behind `bundled-fonts` — `deny.toml` already admits OFL-1.1 — kept out of the wasm default so the 2.5 MB gate holds | M |
| Hinting | outlines are unhinted by design | decision: an autohinter is L and a fidelity question ruling 13 cannot adjudicate; stem darkening is S and measurable as stem width at small pixel sizes; revisit with a corpus of small-text scans | decision |

### Document operations

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| Page labels; embedded files; an outline; `/Info` and XMP; a caller-supplied XMP packet; viewer preferences; trim and art boxes — each **on `DocumentEditor`** | page labels and attachments read only; outline and `/Info` on the builder only; XMP generated only for archival; viewer preferences, trim and art boxes neither read nor written | a typed setter for each, read back by this reader | S each |
| Named destinations on write | deliberately absent: a name is a destination only once the catalog carries a `/Names /Dests` tree | write the tree; ruling 6 still holds, and a named destination is never collapsed | S |
| Optional content: writing groups and configurations | **the reader half landed**: `Document::layers()` returns the catalog's `/OCGs` in the catalog's own order (8.11.4.2), each with its `/Name` and whether the default configuration `/D` shows it (8.11.4.3 Table 101), read from the same bound `OptionalContent` the renderer paints from — so the list and the page cannot disagree. Nothing is written | `DocumentBuilder::add_layer`; the editor toggles a default configuration; a written group read back by `layers()` | M |
| Watermark and stamp on existing pages | `append_content` is the primitive; nothing registers a resource on an existing page | `DocumentEditor::add_resource` and `stamp(page, form)` | M |
| Image recompression and downsampling on rewrite | never, by contract | an opt-in `WriteOptions::images` once the encoders above exist; original bytes untouched by default | M |
| Stream deduplication | declined until content hashing exists | SHA-256 over decoded bytes plus dictionary equality — identical means identical, since a wrong merge silently swaps two fonts | S |
| Sanitise: strip JavaScript, actions, embedded files, metadata | scripts are reported, nothing is stripped | `DocumentEditor::sanitise(Sanitise)` with a typed report of what left | S |
| Encryption on save below R6 — R4 with AES-128, RC4 — for readers that stop at 1.6 | R6 only | decision: the readers that need it are the whole reason | decision |

### Annotations and forms

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| Per-subtype annotation payloads | **the model and its census landed**: `Page::annotations()` types every subtype ISO 32000-1 Table 169 defines and the two ISO 32000-2 adds — 28 variants, and the fetched corpora carry all 28 — with Table 164's and Table 170's common entries beside them (`/Rect` normalised, `/Contents`, `/T`, `/M` as text and as a 7.9.4 date, `/F`, `/Popup`, `/Parent`, whether `/AP` has an `/N`), and 12.5.6.14's pop-up override applied in the one direction the clause states. The list is **total** — one entry per `/Annots` entry, an undefined subtype named as `AnnotationKind::Other`, an unnamed one `Unnamed`, a non-dictionary `Unreadable`. Measured over the corpus at `crates/tinker-pdf/tests/annotation_census.rs`: 14 996 annotations on 982 files, 14 986 covered, 10 refused across 6 subtype names (`FREETEXT` 5, `APEX:Zone`, `SomePrivateCustomAnnotationType`, `line`, one with no `/Subtype`, one that is not a dictionary). What is **not** here is the per-family payload: `/QuadPoints`, `/InkList`, `/Vertices`, `/L`, `/DA`, `/IC`, `/CL`, `/FS`, `/RC` and their relatives are unread. Nor is the 4 096-entry cap reported: past it the list is shortened silently, because a read cannot append to `Document::warnings()` without making the warnings depend on call order — the corpus's largest page is 122, and `a_hostile_annots_array_is_capped` pins the bound | a payload per 12.5.6 family, each held to the corpus count of the subtypes that carry it; and a way for the cap to name what it dropped that does not make a read mutate the document | M |
| AcroForm field creation | signature fields only | `DocumentEditor::add_field` for text, check box, radio and choice, with an appearance | M |
| FDF and XFDF import and export | absent | both directions, held to the field tree this reader builds | S |
| ECMAScript for forms beyond the subset | a deliberate subset under `ScriptPolicy` | decision: a full engine is XL and [design/form-script-policy.md](design/form-script-policy.md) argues against running document code by default; grow the subset by the corpus count of refused constructs instead | decision |

### Signatures

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| A visible signature appearance | an invisible field only; the type says drawing one is a separate capability | an `/AP` from name, date, reason and an optional image, through the appearance synthesis annotations already use | S |
| Timestamps: creating one through a host seam, validating the token | tokens are located and handed out as opaque DER | a `Timestamper` seam like `Signer`, since the engine performs no I/O; validation held to a published token | M |
| Long-term validation: `/DSS` and `/VRI` | absent | written from host-supplied CRL and OCSP bytes | M |
| Public-key encryption on write | a non-goal because the engine would have to choose a certificate — and the caller can supply one | `Encryption::PublicKey { recipients }` sealing with caller-supplied certificates, held to the OpenSSL envelopes this reader already parses | M |

### Standards

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| PDF/UA validation | a measured abstention: 29 of 239 non-conforming fixtures caught, 210 abstained, 0 false alarms over 195 conforming, re-measured 16 September 2026; 24 of the 25 font-clause fixtures abstain, and about half of them ask for rules the PDF/A font group already has and does not run for a PDF/UA claim | a rule group that decides the decidable clauses and abstains by name on the rest | L, schedulable as an M half first ([design/pdfua.md](design/pdfua.md)) |
| PDF/X validation and writing | a `GTS_PDFX` intent is tolerated and never checked; **no annotated PDF/X conformance corpus exists** — the veraPDF corpus carries none, 23 fetched files claim a PDF/X flavour, and the two published suites (Ghent Output Suite 5.0, Altona 1.2) are conforming files only, so a validator would have a false-positive bar and no false-negative bar | an ISO 15930 rule group for the 2003 levels; the archival profile grows a PDF/X flavour; X-4 and X-6 wait on the standards' text | L for the 2003 levels; X-4 and X-6 unpriced ([design/pdfx.md](design/pdfx.md)) |
| PDF/E | absent | decision | decision |
| PDF 2.0: associated files, page-level output intents, namespaced structure types, the UTF-8 string type | encryption is the only 2.0 delta implemented | each read and written, and its row in [pdf20-deltas.md](pdf20-deltas.md) moved | S to M |

### Formats

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| A standalone SVG, a bare image, a loose HTML or XHTML file, each as a document | each refused as not-a-PDF, though the reader for each exists | `Document::open` sniffs and opens them | S each |
| HTML and CSS to PDF as a creation API | the cascade, the layout engine and the painter exist and are wired only inside the EPUB pipeline | `DocumentBuilder::from_html(markup, stylesheet, page box)`, held by the EPUB reftests | M |
| Markdown; FB2 | absent | a hand-written Markdown reader onto the HTML path; FB2 is XML onto the same | S; M |
| MOBI; DOCX | absent | decision: a DOCX layout engine is a word processor | decision |
| Writing EPUB, XPS and CBZ | a decision item already | unchanged | decision |

### Surface

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| A user-facing CLI | nine subcommands, all read-only; the write half of the library is unreachable from it | merge, split, rotate, images, fonts, encrypt, decrypt, sign, attach, stamp, sanitise — each a wrapper over the facade with no logic of its own (ruling 11). **And every one of them that rewrites a document takes the font policy**: `tinker_pdf::write::SaveOptions::fonts` exists and has nowhere on the CLI to attach until this row does, since all nine present subcommands are read-only. That is the surviving half of the closed "Font subsetting on rewrite" row, recorded here rather than left as a twelfth subcommand invented inside a size-S font row, and `docs/features/editing.md` carries it as a named refusal meanwhile. The default follows the facade's: subset | M |
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

Six things the docs called non-goals are rows in tier 5 now, because the
field offers each and the reason given was a scope choice rather than a
limit: hinting, a visible signature appearance, timestamp creation, writing
a public-key-encrypted document, image encoders, and inferred reading order
for untagged pages. Each row says so.

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
