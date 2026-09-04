# JBIG2 symbol dictionary and text region

When this is done, the symbol-dictionary-plus-text-region lineage of ITU-T T.88 —
what `jbig2enc` and OCR pipelines emit, and therefore most JBIG2 in circulation —
decodes through the same `crates/tinker-pdf-filters/src/jbig2.rs` module that
already owns the generic-region lineage, and the engine's highest-reachability
refusal (103 files in the pdf.js corpus, the count the [roadmap](../ROADMAP.md)'s
Tier 2 carried when this was written)
goes to approximately zero in `corpus/ratchet.json`. What still refuses — refinement
initially, halftone permanently for now — refuses for a *narrower named reason*
than today's single `Warning::Jbig2SegmentSkipped`, keeping the
placeholder-plus-warning contract of rulings 2 and 10 in
[../rulings.md](../rulings.md).

## Scope

- **Clause 6.5 symbol dictionary decoding**: the height-class loop, new-symbol
  bitmaps via the generic decoding procedure (6.5.8.1) with one shared context
  set per dictionary, import of symbols from referred-to dictionaries, and the
  export-flag runs of 6.5.10. Arithmetic first; the Huffman variant (SDHUFF,
  including collective bitmaps coded with MMR) staged by measurement.
- **Clause 6.4 text region decoding**: strip decoding with STRIPT and the S/T
  coordinate deltas, symbol ID codes, REFCORNER and TRANSPOSED placement,
  SBDSOFFSET, and the per-symbol combination operator, composited onto the page
  through the existing `Bitmap::composite` (7.4.1.5 semantics already built).
- **Annex A integer arithmetic decoding** (IADH, IADW, IAEX, IAAI, IADT, IAFS,
  IADS, IAIT, IARI, IARDW/RDH/RDX/RDY) and the IAID procedure of A.3, layered on
  the existing `MqDecoder::decode_at` / `MqContexts` in
  `crates/tinker-pdf-filters/src/mq.rs` — the coder is already shared with JPX
  and needs no change.
- **Annex B Huffman tables**: the fifteen standard tables B.1–B.15 built at
  compile time, custom tables from Tables segments (type 53, clause 7.4.13)
  resolved through segment references, and the runcode-assigned symbol ID code
  lengths of 7.4.3.1.7.
- **Clause 6.3 generic refinement region decoding** (GRTEMPLATE 0 and 1, TPGRON)
  as an explicit later stage: it serves refinement-aggregate symbol coding
  (6.5.8.2), SBREFINE in text regions, and segment types 40/42/43. Until it
  lands, these refuse under a new `Warning::Jbig2RefinementSkipped` with a
  reachability test, the pattern `jpx::Refusal` set for coarse warning classes
  backed by a precise internal refusal enum.
- **Segment references**: `read_segment` today skips the referred-to numbers and
  drops the segment number ("nothing this build decodes follows a reference").
  Both are kept — bounded, since the referred bytes must already fit inside the
  header — because a text region's symbol list is the concatenation of its
  referred-to dictionaries' exports in reference order (7.4.3), and custom
  tables are reached the same way. Shared dictionaries in `/JBIG2Globals`
  (ISO 32000-1 7.4.7) need no new plumbing: globals segments are already
  enumerated ahead of the image's own in `decode`.
- **Bounds** in the `crates/tinker-pdf/tests/bounds_ledger.rs` style: symbol
  count, total dictionary pixel budget, and text-instance count, each a named
  constant checked before allocation (the `packed_size` pattern) and each a
  ledger row measured against real OCR output rather than guessed.

## Non-goals

- **Halftone regions and pattern dictionaries** (clauses 6.6, 6.7; segment
  types 16, 20, 22, 23) — a third lineage, still out of scope, but **not for the
  reason first given here.** "Near-zero corpus presence" was wrong: the census
  finds 50 such segments across 16 files, and every one of those 16 carries
  halftone *and nothing else*, so all sixteen still refuse under
  `Jbig2SegmentSkipped` when all eight milestones below are done. That is a
  sixth of the JBIG2 in the corpus, and it is a roadmap item this work does not
  close rather than a lineage nobody emits.
- **Colour palette segments** (type 54) and the T.88 amendment features
  (EXTTEMPLATE, colour extension) — not emitted by any encoder the corpus sees.
- **Retained bitmap-coding contexts across segments** (the used/retained flags
  in the symbol dictionary flags field, 7.4.2) — the census was to decide this
  and the count did **not** stay zero: one file uses a retained context, three
  segments consuming and two retaining. One file is thin evidence for the
  machinery, so it stays out of scope and refuses by name — but by name, with a
  reachability test, rather than as something believed absent.
- **Encoding.** The `MqEncoder` in `mq.rs` stays test-only, used to build
  fixtures the way the generic-region tests already do.
- **The random-access file organisation** (Annex D.1) — already refused in
  `segments()`; unchanged.
- No new crate, no new public API: `jbig2_decode` keeps its exact signature,
  bytes-in/values-out per ruling 8, and the fuzz target's contract — success
  returns exactly the page the caller sized, failure is exactly
  `FilterError::Unsupported(Capability::Jbig2)` — is preserved.

## Design

**Where it lives.** Everything goes into `jbig2.rs` behind the existing seams.
`understood()` gains types 0, 4, 6 and 7; `carries_content()` shrinks by the
same set, which is what narrows the warning without touching any caller. `Page`
gains a symbol store — a `BTreeMap<u32, Vec<Bitmap>>` keyed by segment number,
`BTreeMap` for deterministic iteration per ruling 4 — filled by
`kind::SYMBOL_DICTIONARY` and read by the text-region arm of the dispatcher.
A text region whose referred-to dictionary is missing or refused draws nothing
and does not count a region, so the existing `page.regions == 0` refusal at the
end of `decode` keeps catching whole-file failure; a *partial* page (one region
of several missing) degrades with the warning, exactly the generic-region
truncation bargain.

**Reuse, named.** Per-symbol bitmaps decode through the existing
`decode_arithmetic` with the dictionary's own `MqContexts` living across
symbols (6.5.8.1 requires the shared adaptive state). The Huffman variant's
collective bitmaps reuse `crate::T6Rows` exactly as `decode_mmr` does — one
T.6 implementation in the crate, and the 1-black polarity already agrees.
Placement math accumulates CURS/CURT in `i64` and clips through
`Bitmap::composite`'s existing silent-clip contract, so no coordinate on the
wire can index out of the page.

**Measured before staging** (ruling 3), and the measurement disagreed with the
expectation. The census is `crates/tinker-pdf/tests/jbig2_census.rs`, an
`#[ignore]`d test that walks every JBIG2 stream in the fetched corpora and
tallies the flags — with a segment-header reader **written from clause 7.2 and
sharing nothing with `jbig2.rs`**, because a census taken with the decoder's own
reader would agree with that reader by construction, including wherever it is
wrong.

102 files carry JBIG2. 43 of them carry this lineage; the other 59 are generic
regions, and 16 of those are halftone or pattern segments and nothing else.

| | segments | files |
| --- | ---: | ---: |
| symbol dictionaries | 59 | 43 |
| text regions | 58 | 43 |
| refinement regions | 40 | 15 |
| halftone / pattern | 50 | 16 |
| custom tables (type 53) | 21 | 6 |
| SDHUFF | 18 | 15 |
| SBHUFF | 15 | 15 |
| SDREFAGG | 8 | 8 |
| SBREFINE | 13 | 13 |
| refinement regions by template (0 / 1) | 37 / 3 | 15 |
| refinement regions with TPGRON (6.3.5.6) | 5 | 3 |
| context used / retained | 3 / 2 | 1 |

The scheduling question is not how many segments use a feature but **how many
files a stage unlocks**, which is a question about overlap — a file needing both
Huffman and refinement is unlocked by neither alone. Of the 43:

| stage | unlocks | cumulative |
| --- | ---: | ---: |
| arithmetic alone (milestones 3–4) | 19 | 19 |
| then Huffman | 5 | 24 |
| then refinement | 9 | **28** |
| needs both | 10 | 43 |

The same census was extended before milestone 4 was written, because clause
6.4's placement rules are the part of this item where a wrong guess decodes
*plausibly* rather than visibly, and which of them a file uses is a question
about files. Over the 58 text regions:

| | regions | files |
| --- | ---: | ---: |
| REFCORNER top-left | 48 | |
| REFCORNER bottom-left | 4 | |
| REFCORNER bottom-right | 3 | |
| REFCORNER top-right | 3 | |
| more than one strip (LOGSBSTRIPS > 0) | 55 | 43 |
| SBDSOFFSET non-zero | 25 | 24 |
| TRANSPOSED | 4 | 4 |

So milestone 4 owes all four corners, the strip coordinate and SBDSOFFSET —
multi-strip is the norm rather than the exception, and a build that assumed one
strip would mis-place almost every symbol in the corpus. TRANSPOSED is the one
placement variant thin enough to stage: four regions in four files, and it
refuses by name until it is built.

**So refinement comes before Huffman, and milestones 5 and 6 are swapped from
the order this document first proposed.** The expectation was
arithmetic ≫ Huffman ≫ refinement, on the grounds that `jbig2enc` emits only
arithmetic coding; the corpus is not all `jbig2enc`, and refinement unlocks
nearly twice what Huffman does. That is what milestone 1 was for.

**One thing the annex does not give.** H.1's datastream is committed byte for
byte and its *generic region* picture is transcribed beside it, but its symbol
bitmaps are not, and they cannot be invented. So milestone 3's symbols are
pinned against a dictionary this repository encodes — a real round trip over
clause 6.5, and weaker than the standard's own answer in exactly the way ruling
13 warns about, since both sides share one reading. Milestone 4 is where the
annex adjudicates again: its text region composites those symbols onto a page,
and the page is published.

**Conformance fixtures already committed.** The `ANNEX_H` constant in
`jbig2.rs`'s tests is the whole T.88 Annex H.1 datastream, byte for byte, and
its pages carry symbol dictionaries and text regions in both coding variants —
today asserted as *skipped* (`a_symbol_dictionary_file_refuses_and_says_so`,
and the page-window tests that prove the text region was not drawn). Each
milestone flips the corresponding negative assertion into a pixel-for-pixel
positive one against the bitmaps the annex publishes, the same discipline the
generic-region work used. Where H.1 codes one picture two ways, the two decodes
must agree exactly — the cross-check that already pins MMR against arithmetic.

**Refusal narrowing** (rulings 2, 10). A `pub(crate) enum Refusal` in `jbig2`
mirrors `jpx::Refusal`: precise conditions internally, the closed `Warning` set
outwardly. New variants: `Jbig2RefinementSkipped` and `Jbig2SymbolLimitHit`;
each must be reachable and is asserted reachable by a decoding test, the
`jpx::tests::refusals` pattern. `Jbig2SegmentSkipped` survives for what stays
outside scope.

**Verification** ([../verification.md](../verification.md)). Fuzzing: the
existing `fuzz/fuzz_targets/jbig2.rs` needs no new knobs — its assertions
(exact output size, exact refusal error, warning dedup, swapped globals) are
already the right contract — but its seed corpus gains symbol/text fixtures
written by an `#[ignore]`d test in `jbig2.rs`, the house pattern that keeps
seeds and fixtures from drifting. Ruling 13 rules out asking another decoder
what a page should look like, so the anchor is the standard itself: T.88's
Annex H.1 publishes a page bitmap for both coding variants, which is the
artefact this whole capability is tested against, and the ratcheted
`cargo xtask corpus-run` measures the hit-rate exit.
Determinism: integer-only decoding, no `HashMap`, and one JBIG2-bearing render
fingerprint joins `crates/tinker-pdf/tests/determinism.rs`. Injection: at least
one deliberate defect per decoder (a transposed IAID context, a wrong refinement
template bit) with the catching assertions counted, as the TPGDON-context
injection at the bottom of `jbig2.rs` already does.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size (S/M/L/XL) |
|---|---|---|---|
| 1 | Corpus census of the JBIG2 files | **Done.** `crates/tinker-pdf/tests/jbig2_census.rs` prints per-file SDHUFF/SBHUFF/SDREFAGG/SBREFINE/retention tallies; numbers recorded above; milestones 5 and 6 swapped by them | S |
| 2 | Segment references + Annex A integer decoders | **Done.** `Segment` carries its number and referred-to list; `every_integer_range_and_oob_round_trips` covers all six A.2 fields at both ends and OOB, `the_symbol_index_procedure_round_trips_at_every_code_length` covers A.3 including `SBSYMCODELEN` zero; the committed fuzz seeds are replayed on stable by `tests/jbig2_seeds.rs` | M |
| 3 | Symbol dictionary, arithmetic, SDREFAGG=0 | **Done**, with one exit criterion changed and the change stated: symbols round-trip pixel-for-pixel against an `MqEncoder`-built dictionary over three height classes and all four templates; 6.5.10's export runs select across imported and new symbols; the variants this build declines refuse under `Jbig2VariantSkipped`. Annex H.2's *published symbol bitmaps* are not in this repository — H.1's datastream is, its generic-region picture is, its symbol pictures are not — so the pixel-for-pixel claim is against a fixture this repository builds rather than against the standard. Milestone 4 recovers the standard's own adjudication: the annex's text region composites those symbols into a page, and that page can be compared with what H.1 publishes | M |
| 4 | Text region, arithmetic (SBREFINE and TRANSPOSED refused by name) | **Done**, with the exit criterion changed and the change stated below: `a_text_region_places_its_symbols_where_6_4_5_computes` places symbols across two strips at the coordinates 6.4.5 computes, through a round trip; `a_text_region_whose_dictionary_refused_is_refused_by_name` holds a region whose referred-to dictionary is absent or refused to refusing **whole**; all four reference corners handled, SBDSOFFSET and multi-strip regions decoded. The annex's own page moves to milestone 6 | M |
| 6 | Annex B Huffman: standard tables, 6.5.9's collective bitmaps, 7.4.3.1.7's symbol-ID codes | **Done.** `annex_h_codes_the_same_symbols_two_ways` holds the reconstructed tables to the annex's arithmetic twin; 44 to 47 files clean | M |
| 5 | Clause 6.3 refinement + 6.5.8.2 aggregate + SBREFINE + segment types 40/42/43 — **ahead of Huffman, by milestone 1's census: 9 files against 5** | **Done**, and with the exit criterion changed twice; both changes stated above. The `MqEncoder` round trip was dropped for Annex H page 3, and Annex H alone was then found insufficient — the corpus's own ground truth is what settles the templates. Landed on: page 3 decodes into its refined text with no warning; all ten corpus refinement variants reproduce the unrefined picture with 0 pixels different; `a_relabelled_refinement_template_decodes_identically` pins the argument the templates rest on. Counted injection below | M |

### Milestone 6 landed, and what actually adjudicates it

Annex B's tables are **reconstructed rather than transcribed** — this repository
has no copy of T.88 — so the question is what holds them to the standard. Two
things turned out to be true, and one thing that was expected to be turned out
not to be.

**The dictionary has a real oracle.** Annex H codes the same two symbols twice:
segment 2 carries them with `SDHUFF = 1`, through height classes, a collective
bitmap and Tables B.1, B.2 and B.4; segment 9 carries them with `SDHUFF = 0`,
through the MQ coder and Annex A's integer decoders. The two share no code below
`Segment`, and they decode to **byte-identical** glyphs — a 'c' and an 'a', six
by six. A prefix length wrong by one bit desynchronises the reader and yields
noise, not a glyph, so this is the standard adjudicating the reconstruction.
`annex_h_codes_the_same_symbols_two_ways` is that assertion.

**The text region does not.** The plan was to lean on the whole-page comparison
in `annex_h_codes_one_picture_twice_and_both_ways_agree`, on the reading that
Annex H codes one picture as a Huffman page and an arithmetic page. It does not:
pages 1 and 2 draw *different text* from their symbols. That was invisible until
now because **both** pages' text regions were being skipped, so the whole-page
assertion was comparing the generic region and two identical expanses of white
— a vacuous claim that read as the strongest one in the file. It has been
narrowed to what is actually true, which is that the generic region is coded
both ways and both match the published picture.

So B.6, B.8 and B.12 rest on weaker evidence than B.1, B.2 and B.4: the region
decodes without error, places symbols of the right dimensions at varying
baselines, and renders as legible glyphs rather than noise — which a wrong table
would not survive, but which is not pixel-exactness. Said plainly here because
the difference between the two halves is real and a reader should not have to
infer it.

**What it bought, measured over the 104 JBIG2-bearing corpus files:** 44 clean
before, **47 after**, and files reporting `Jbig2VariantSkipped` down from 29 to
25. The census predicted five; three arrived, because two of the five need
refinement as well and are unlocked by neither stage alone — which is the
overlap the census exists to show and the reason it counts *files* rather than
segments.

### What a bound has to clear, and why milestone 7 cannot measure it here

Milestone 7 asks for `MAX_JBIG2_SYMBOLS`, `MAX_JBIG2_SYMBOL_BYTES` and
`MAX_JBIG2_TEXT_INSTANCES` as ledger rows "each measured against a real
`jbig2enc`/OCRmyPDF output". The census was extended to take that measurement,
reading `SDNUMNEWSYMS`, `SDNUMEXSYMS` and `SBNUMINSTANCES` off the segment
headers of all 102 JBIG2-bearing files. What it found:

| | largest in the corpus |
| --- | ---: |
| `SDNUMNEWSYMS` | **8** |
| `SDNUMEXSYMS` | **11** |
| `SBNUMINSTANCES` | **9** |

Nothing in 102 files exceeds twenty of anything. A real OCR page carries
hundreds of distinct symbols and thousands of instances, so **the corpus
contains no real OCR JBIG2 at all** — every file in this lineage is a synthetic
fixture, overwhelmingly pdf.js's own `bitmap-symbol-*` family, built to
exercise one placement variant each. That is why they were so useful for
milestone 4's REFCORNER and SBDSOFFSET coverage, and it is exactly why they are
useless as a yardstick.

So milestone 7's exit criterion cannot be met as written, and saying so is
better than quietly satisfying it against files that would let any cap through.
The rows must be **arithmetic about a plausible file, written down so it can be
argued with** — which is what `bounds_ledger.rs`'s own header says its comic
and document yardsticks already are — rather than a measurement dressed up as
one. The arithmetic to argue with: a 300 dpi A4 text page reduces to a few
hundred distinct glyph bitmaps and a few thousand placements, so a 200-page
document sharing one global dictionary reaches the low tens of thousands of
symbols and the low thousands of instances per region.

Getting a real measurement needs a file this repository does not have. Adding
one means either committing OCR output — which `corpus/README.md`'s licensing
position rules out for the same reason it rules out committing any corpus — or
fetching a corpus that carries some. Neither is milestone 7's business to
decide, so the rows land as stated estimates and the gap is named here.

### Milestone 5 landed, and the route to it is the point

Clause 6.3 was written and run against Annex H page 3, on the reasoning that a
wrong context template desynchronises the arithmetic decoder, the integer
decoders after it return nonsense, and the strict count checks then refuse the
segment — so the experiment could only ever cost a refusal, never a wrong
picture. The first run produced `REFAGGNINST = -12` on the second symbol and
was recorded here as *blocked on 6.3.5.3's two figures*. **That diagnosis was
wrong, and the way it was wrong is worth keeping.**

**The defect was `SBSYMCODELEN`, not the template.** 6.5.8.2.3 makes the symbol
code as wide as the whole dictionary needs — `SDNUMINSYMS + SDNUMNEWSYMS`, so
two bits for Annex H's one imported and two new symbols — and the first
attempt sized it against the symbols decoded *so far*, which is one bit. One
extra `IAID` decision per symbol, and everything after the first refinement is
noise. It presented exactly as a wrong context template would, which is why it
was misread as one.

#### The bit order in 6.3.5.3's figures does not have to be known

A context index names an adaptive state slot and nothing else: the decoder
reads and writes `state[cx]`, every slot begins in the same state, and the MQ
coder's A and C registers are global. **Relabel every context through any
bijection and the decision sequence is bit-for-bit unchanged** — each slot is
still reached by exactly the same neighbourhoods in the same order. So the
order the standard's figures put the pixels in is unobservable, and only the
*set* of positions has to be right. `a_relabelled_refinement_template_decodes_identically`
is that argument as a test: the same thirteen positions in two different orders
over the same bytes, asserted to produce the same bitmap.

The one thing that does not survive the relabelling is 6.3.5.6's TPGRON
pseudo-context, which is a bare number in the standard's own ordering. It is
recovered below rather than transcribed.

#### Annex H alone is not enough, and it is instructive that it looks like it is

Page 3's dictionary refines a single 6 by 6 symbol: thirty-six coded decisions.
Searching every 10-subset of a 21-position pool against page 3's text region,
with two acceptance conditions — the counts all check out, and each refinement
stays within 20 % of its reference, since a refinement is by construction a
small edit — left **five** candidate templates for `GRTEMPLATE 1`. All five
decoded page 3 into the same legible line of text.

All five were wrong. They render the third glyph without its descender. Thirty-
six decisions is simply not enough information to separate them, and the
agreement between them reads as corroboration when it is nothing of the kind.
Had any one been picked on that evidence it would have shipped as a decoder
that is right on one fixture and silently wrong everywhere else — the exact
failure this module says is worth less than a refusal.

#### The corpus holds the twin the annex does not

The pdf.js fixtures are one 399 by 400 bitmap encoded a dozen ways — four
generic templates, MMR, TPGDON, custom AT, a symbol dictionary, and eight
refinement variants — each a lossless encoding of the same picture. **The files
that do not refine are therefore ground truth for the files that do**, and the
comparison is between two of this engine's own decodes, so ruling 13 is
untouched. That is the cross-check this document previously said did not exist;
it said so having looked only in Annex H.

Against a whole picture the answer is immediate and unique:

| what was unknown | how it was settled |
| --- | --- |
| `GRTEMPLATE 0`'s thirteen positions | Annex H page 3's dictionary refines 'a' into a legible 'c'; `bitmap-refine-page.pdf` then reproduces the corpus picture exactly |
| `GRTEMPLATE 1`'s ten positions | `bitmap-refine-template1.pdf`, 0 pixels different over 159 600 |
| 6.3.5.6's TPGRON slot, template 0 | searched all 8 192; **exactly one** reproduces `bitmap-refine-tpgron.pdf` |
| 6.3.5.6's TPGRON slot, template 1 | searched all 1 024; **exactly one** reproduces `bitmap-refine-template1-tpgron.pdf` |

`bitmap-refine-customat.pdf` pins the adaptive pair to the destination and
reference layers the right way round, and `bitmap-refine-customat-tpgron.pdf`
is worth naming as a fixture that proves nothing on its own: 8 039 of the 8 192
slots reproduce it, because its typical rows never fire. A fixture that passes
under most values of an unknown is not evidence about that unknown, and the
whole difference between this section and the one it replaced is noticing that.

#### Two defects the refinement work found in code that was already there

- **An intermediate text region was being drawn.** 7.4.6.1 says an intermediate
  region waits for the segment that refers to it; `Page::draw_text` composited
  it onto the page anyway. Invisible until a refinement region referred to one,
  at which point the page carried the picture twice — once unrefined —
  and `bitmap-symbol-refine.pdf` came out 291 pixels heavy.
- **Intermediate generic regions were skipped entirely**, so the ordinary
  `bitmap-refine.pdf` shape — code a region, then refine it — had nothing to
  refine against.

#### 6.5.8.2.2's reference offset, and what a fixture there can and cannot say

`refinement_offset` implements one of two readings of the clause: **split the
size difference, then add `RDX`** — the same arithmetic 6.4.11 uses, which is
why one function serves both roads. The other reading is `RDX` alone. The two
coincide exactly when the refined symbol is its reference's size, which is true
of Annex H's refining dictionary and of every corpus file that reaches that
road, so nothing in the tree could choose between them on the dictionary road
and this section said so.

`a_refined_symbol_that_is_not_its_reference_s_size_pins_6_5_8_2_2` is the fixture
that can. It refines one 6 by 6 symbol into an 8 by 6 and then into a 5 by 7,
both at `RDX = RDY = 0`, so the split term *is* the offset: 1 for the wider
symbol and −1 for the narrower. Three arithmetics put the reference in three
different places there — the split, `RDX` alone, and a truncating `/2`, which
parts company with `div_euclid` only when the difference is negative and odd —
and the assertion is the two decoded pictures, because a reference one column
out desynchronises the coder and everything after it is noise rather than a
slightly wrong picture.

**What that is worth, stated exactly.** The encoder is in this repository, so a
round trip cannot say which reading is T.88's. `split_offset` in the test module
is written out separately from `refinement_offset` precisely so that an
injection into the decoder is not silently mirrored by the encoder — the same
separation `INT_RANGES` keeps from `decode_int`, and without it the fixture
passes under every reading and proves nothing. What it buys is that the choice
is now **load-bearing** where it was not: before it, changing `refinement_offset`
to either alternative broke nothing on the dictionary road at all.

The counted table below is the measurement, and it also shows where the
standard's own datastream does bear on the question. Under `RDX` alone, Annex H
page 3's *text region* stops decoding altogether — the published stream refuses,
because a reference in the wrong place desynchronises the coder the integer
decoders share and the counts then fail — and under a truncating `/2` it fails
as well. So 6.4.11's arithmetic was adjudicated by published data all along;
what was unpinned was **6.5.8.2.2 sharing it**, on a road no fixture reached at
a size difference. That is the narrower and true statement, and the fixture is
what closes it.

#### What is still not pinned, stated rather than absorbed

- **Huffman refinement, half built.** 6.4.11's envelope — `RI` as one plain
  bit, the four deltas through a selected table, `BMSIZE`, byte alignment, and
  an arithmetic sub-stream the bit reader steps over — is **done for text
  regions**, and with it tables **B.14 and B.15**.
  `bitmap-symbol-texthuffrefine.pdf` selects B.14 and
  `bitmap-symbol-texthuffrefineB15.pdf` selects B.15, and both reproduce the
  unrefined picture with 0 pixels different.

  **But that adjudicates the envelope, not the tables**, and it took counted
  injection to notice. Both files code every delta as zero, so the only line
  either table exercises is its one-bit code for 0: changing a prefix length,
  a range low or a range length anywhere else in either table breaks nothing
  in the tree. This is the same trap as the five refinement templates that
  agreed with each other, met a second time and caught by a different tool —
  a fixture that passes under most values of an unknown says nothing about
  that unknown.

  So the reconstruction is not trusted past its evidence. The zero line being
  one bit makes "is this delta zero" a single-bit question no error elsewhere
  can affect, and `text_region_procedure` **refuses the segment if any delta
  is non-zero**. That costs nothing measurable — no corpus file has one — and
  it turns an unverified table into a refusal rather than a picture.

  **The dictionary road (`SDHUFF` with `SDREFAGG`) decodes too**, through both
  6.5.8.2.2's single refinement and 6.5.8.2.1's aggregate, and all three
  `symhuff*` corpus files reproduce the picture with 0 pixels different.

  #### The entry this replaces was wrong, and how it was wrong is the point

  It said the blocker was a defect in 6.5.9's Huffman collective path, on two
  observations. Both were misreadings:

  - *"Two symbols of identical width inside one height class."* 6.5.5
    accumulates `SYMWIDTH` by a delta that may be **zero**, so two symbols of
    the same width in one class is ordinary — and the arithmetic encoding of
    the same picture, which has always decoded at 0 pixels different, exports
    two identical 30 by 30 symbols itself. The observation was true and meant
    nothing.
  - *"A symbol with no ink at all."* Deliberate. That class's collective bitmap
    really is blank across its right-hand 59 columns, because the symbol exists
    **to be refined into content** — which is precisely what the fixture is
    testing.

  The actual defect was in the refinement road after all, and it was one line:
  **6.3's adaptive states were reset for every symbol.** 6.5.8.1 says a
  dictionary carries one set of states from one symbol to the next, and that is
  as true of refinement as of the generic procedure — the *coder* restarts at
  each byte-aligned sub-stream because the stream does, but the statistics do
  not. Resetting them decodes the first refined symbol correctly and every one
  after it as noise.

  #### Three bugs, one symptom

  That is the third distinct defect in this lineage to present as *"the
  refinement context template must be wrong"*: first `SBSYMCODELEN` sized
  against the symbols decoded so far, then the five candidate templates that
  agreed with each other on a thirty-six-decision fixture, and now these reset
  states. **The symptom is not diagnostic.** A desynchronised arithmetic
  decoder produces noise whatever pushed it out of step, and every one of the
  three was found by a picture rather than by reasoning about the template —
  which is the argument for keeping a fixture that renders a whole page
  wherever a decoder has an internal state a header cannot check.

- **Custom code tables (clause 7.4.13, type 53)** are the other half of the
  Huffman refinement files: five of the eight refining Huffman text regions
  select a custom table for their deltas or their size, and are refused for
  that rather than for the refinement.

#### Counted injection, and what it says about where the evidence lives

Each row is one fact perturbed and the whole JBIG2 suite re-run, counting
assertions that fail. `filters` is `cargo test -p tinker-pdf-filters jbig2`;
`corpus` is `jbig2_refinement.rs`, which is `#[ignore]` and runs from
[`corpus.yml`](../../.github/workflows/corpus.yml).

| perturbation | filters | corpus | total |
| --- | ---: | ---: | ---: |
| template 0, one reference position moved | 3 | 2 | 5 |
| template 0, one destination position moved | 3 | 2 | 5 |
| template 1, one reference position moved | 2 | 2 | 4 |
| 6.3.5.6's TPGRON slot, template 0, off by one | **0** | 2 | 2 |
| 6.3.5.6's TPGRON slot, template 1, off by one | **0** | 2 | 2 |
| `SBSYMCODELEN` sized against the symbols so far | 3 | 0 | 3 |
| an intermediate text region composited after all | **0** | 2 | 2 |
| 6.4.11: `RI` read through a table rather than as one bit | 0 | 2 | 2 |
| 6.4.11: the reader does not step over `BMSIZE` | 0 | 2 | 2 |
| 6.4.11: the sub-stream is not byte-aligned | **0** | **0** | **0** |
| B.14 or B.15, any line but the one-bit code for zero | **0** | **0** | **0** |
| 6.5.8.1: 6.3's states reset per symbol, Huffman dictionary | 0 | 2 | 2 |
| the same, arithmetic dictionary | 1 | **0** | 1 |
| 6.5.8.2.2's reference offset: `RDX` alone | 2 | **0** | 2 |
| 6.5.8.2.2's reference offset: `/2` rather than `div_euclid(2)` | 2 | **0** | 2 |

The arithmetic dictionary's row was 0 in both columns until the offset fixture
above, and why is worth naming: Annex H's refining dictionary decodes one symbol
through 6.5.8.2.2 and one through 6.5.8.2.1, so the *first* refinement is the
only one that road ever takes on its own — a reset it cannot notice, because
there is nothing yet to carry. The new fixture refines two symbols in a row for
exactly that reason, and the row is 1 rather than 0 because of it.

**Two rows are still caught by nothing at all**, and they are the most
useful lines in the table: they are what turned a claim that the corpus
adjudicated B.14 and B.15 into the narrower and true claim that it adjudicates
6.4.11's envelope. Both fixtures happen to sit on a byte boundary and code
every delta as zero, so neither the alignment nor any table line past the zero
code is exercised. The refusal of non-zero deltas above exists because of these
two rows; the alignment is left in because it is what the clause says and
nothing depends on a guess about it.

**Three more are caught by nothing except the corpus gate**, and those zeros
matter too. Annex H carries no
refinement region segment at all, so it cannot exercise TPGRON, and it carries
no intermediate region, so it cannot notice one being drawn. That is why
`corpus.yml` runs those assertions explicitly rather than leaving them to a
`--ignored` nobody passes: without that step three of these facts would be held
by a comment.

### Milestone 5 has an anchor, and it is better than the round trip

Annex H.1 carries no refinement *region* segment — a walk of its twenty-one
segments finds none of types 40, 42 or 43 — but **page 3 is a refinement
fixture in both of the other two shapes**. Segment 17 is a symbol dictionary
with `SDREFAGG=1` importing from segment 16, which is 6.5.8.2's
refinement/aggregate symbol coding at `GRTEMPLATE=0`; segment 18 is a text
region with `SBREFINE=1` at `GRTEMPLATE=1`. So the standard's own datastream
exercises clause 6.3 through **both** templates, and the fixture is already
committed to this repository.

The exit criterion should therefore be page 3 decoding, not an
`MqEncoder`-built fixture round-tripping against an encoder that mirrors the
decoder's own reading. That is the same correction milestones 3 and 4 each had
to make after the fact; this one is available before the code rather than
after it.

**This paragraph used to say the milestone was blocked on 6.3.5.3's two
figures**, on the grounds that a template wrong by one pixel does not fail —
the arithmetic decoder desynchronises and returns *a picture* — which is
precisely the failure this module's refusals exist to prevent. The first half
of that is right and worth keeping. The conclusion drawn from it was not: the
figures were never obtained and were never needed.

Two things unblocked it, both recorded above. A context index labels an
adaptive state slot, so the *order* in those figures is unobservable and only
the set of positions is a fact about the format; and the corpus codes one
picture with refinement and without it, which adjudicates the set far more
sharply than Annex H's thirty-six decisions can. What the old paragraph got
right is why the second of those is not optional: five templates passed the
Annex H test and were all wrong, so "it decodes and looks like a picture" is
exactly as weak as this paragraph always said it was.

| 6 | Huffman variants: Annex B tables, type-53 custom tables, 7.4.3.1.7 symbol IDs, MMR collective bitmaps via `T6Rows` | H.1's Huffman-coded page decodes pixel-identical to its arithmetic twin; an over-subscribed custom table refuses with an asserted warning | M |
| 7 | Bounds and fuzz hardening | `MAX_JBIG2_SYMBOLS`, `MAX_JBIG2_SYMBOL_BYTES`, `MAX_JBIG2_TEXT_INSTANCES` rows in `bounds_ledger.rs`, each measured against a real `jbig2enc`/OCRmyPDF output and none refusing it; a recorded fuzz session over the extended seeds with zero crashes | S |
| 8 | Corpus closure and docs | `cargo xtask corpus-run` shows `Capability::Jbig2` hit-rate ~0 in `ratchet.json`; every JBIG2-bearing corpus file renders without a placeholder warning, counted; one JBIG2 fingerprint in `determinism.rs`; the symbol/text refusal rows leave [../features/filters.md](../features/filters.md) | S |

### What Annex H.1 cannot adjudicate, and when it can

Milestone 3's row promised that *"milestone 4 recovers the standard's own
adjudication: the annex's text region composites those symbols into a page, and
that page can be compared with what H.1 publishes."* It does not, and the reason
is worth recording rather than working around.

Annex H.1's page 2 codes its text region arithmetically — segment 10 — but that
segment refers to segments **0 and 9**, and segment 0 is *page 1's Huffman
symbol dictionary*. 7.4.3 numbers a text region's symbols across the
concatenation of every dictionary it refers to, in reference order, so with
segment 0 refused the numbering is short by that dictionary's exports and every
instance would draw a different symbol at the right place. The annex's
arithmetic page depends on the Huffman variant, so its picture arrives at
**milestone 6**, not here — and that is an argument for Huffman that the
file-count tally does not make on its own.

What this milestone lands instead is the refusal that situation demands, plus a
round trip for the placement plumbing. Both are honest about what they are:

- **The refusal is the load-bearing half.** A region whose referred-to
  dictionary is absent or refused is refused whole, because drawing it
  renumbered produces a page that looks like text and says something else —
  the failure mode this lineage has that a generic region does not. It is
  reached by the annex's own page 2, which is the fixture.
- **The round trip proves plumbing, not convention.** Both sides share one
  reading of 6.4.5, so it shows the strip coordinate accumulating, the
  out-of-band value ending a strip rather than the region, the gap being
  measured from the previous symbol's far edge rather than its origin, and the
  symbol code being as wide as the count needs. It cannot show that the
  placement convention itself is right. Nothing here can until milestone 6.

### What the corpus said, which is better than either

Measured after the milestone landed, over the 102 JBIG2-bearing corpus files,
counting files whose render reports **any** JBIG2 warning (`tpdf probe`, 72 dpi,
August 2026):

| | files |
| --- | ---: |
| before | 65 |
| after | **52** |
| newly clean | 13 |
| newly warning | 0 |

The thirteen are worth listing, because of what they are named:

```
bitmap-symbol.pdf                     bitmap-symbol-textbottomleft.pdf
bitmap-symbol-textcomposite.pdf       bitmap-symbol-textbottomright.pdf
bitmap-symbol-big-segmentid.pdf       bitmap-symbol-texttopright.pdf
bitmap-symbol-negative-sbdsoffset.pdf issue17871_bottom_right.pdf
bitmap-composite-and-xnor-text.pdf    issue17871_top_right.pdf
bitmap-composite-or-xor-replace-text.pdf  jbig2_symbol_offset.pdf
issue20439.pdf
```

These are documents nobody here authored, written by somebody else to exercise
precisely the parts of 6.4.5 that had to be derived rather than read: **three of
the four reference corners by name**, a negative `SBDSOFFSET`, the combination
operators, and a segment number wide enough to widen its own referred-to fields.
A decoder that had the corner convention backwards would still produce no
warning on them — so this is not proof — but it is a great deal better than a
round trip against an encoder sharing one reading, and it is the strongest
adjudication available before the annex's page arrives at milestone 6.

The 52 that remain are the ones the census predicted: Huffman, refinement, and
the halftone lineage that is a non-goal.

One thing about the convention *is* settled by reading rather than by fixture,
and it removes half the risk: 6.4.5 advances the running coordinate past the
symbol's width **before** drawing for the two right-hand reference corners and
**after** drawing for the two left-hand ones. Both orders leave the symbol's
left edge at the value the coordinate held on entry and leave the coordinate at
the symbol's far edge, so the horizontal placement is corner-independent and
only the vertical coordinate branches on TOP versus BOTTOM. The census found all
four corners in use — TOPLEFT in 48 of 58 regions, BOTTOMLEFT in 4, and three
each of the right-hand pair — so none of them could have been skipped.

## Dependencies

- `crates/tinker-pdf-filters/src/mq.rs` — `MqDecoder`, `MqContexts`,
  test-only `MqEncoder`; shared with JPX, unchanged.
- `crates/tinker-pdf-filters/src/jbig2.rs` — `Segment`, `Reader`, `Bitmap`,
  `RegionInfo`, `packed_size`, `Page`, the `ANNEX_H` fixture, and the
  `Warning`/`Capability` enums in `lib.rs`.
- `crate::T6Rows` (the T.6 decoder `decode_mmr` already reuses) for
  Huffman-variant collective bitmaps.
- Corpus infrastructure: `cargo xtask corpus-fetch` / `corpus-run` and
  `corpus/ratchet.json`. Ruling 13 means milestone 8 measures the hit-rate
  and the absence of placeholder warnings rather than comparing pixels with
  anything; T.88 Annex H.1 carries the pixel-exact weight instead.
- `crates/tinker-pdf/tests/bounds_ledger.rs` (ledger rows) and
  `crates/tinker-pdf/tests/determinism.rs` (fingerprint).
- `fuzz/fuzz_targets/jbig2.rs` and its committed seed corpus.

## Risks

| Risk | Mitigation |
|---|---|
| Context-lifetime bugs (dictionary contexts shared across symbols, IAID width derived from symbol count) decode plausibly wrong pages rather than crashing | Annex H.1 pixel-exact assertions in both coding variants — the standard's own bitmap, which under ruling 13 is the only adjudicator there is — plus injection tests with counted catches. Beyond H.1's page the corpus can say a file decoded, not that it decoded correctly, and this doc says so |
| Symbol dictionaries invite allocation blowup: 32-bit symbol counts, per-symbol bitmaps, per-strip instance counts | Every count capped by a named budget checked before allocation (`packed_size` pattern); budgets are ledger rows measured against real OCR files so no cap refuses the thing the format is for |
| Huffman effort wasted if the corpus is all-arithmetic — or arithmetic-first wrong if it is not | Retired by milestone 1: the corpus is *not* all-arithmetic. 19 of 43 files need arithmetic alone, and the other 24 need refinement, Huffman or both. Arithmetic first is still right; Huffman last rather than second |
| Dangling references: text region whose dictionary is in a missing globals stream, or refused over budget | Region not counted; page with zero regions still refuses by name; the fuzz target already swaps globals and own streams every run |
| Signed placement arithmetic (S/T deltas, REFCORNER, TRANSPOSED) indexing off-page | `i64` accumulation, clipping only through `Bitmap::composite`'s existing bounds-checked path; fuzz asserts exact output size on every success |
| Refinement staged late leaves a residual refusal that looks like the old one | `Jbig2RefinementSkipped` is a distinct warning with a reachability test from milestone 4, so the residual is named and measured, not folded into `Jbig2SegmentSkipped` |
