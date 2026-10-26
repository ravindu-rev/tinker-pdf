# JBIG2 symbol dictionary and text region

The symbol-dictionary-plus-text-region lineage of ITU-T T.88 — what `jbig2enc`
and OCR pipelines emit, and therefore most JBIG2 in circulation — decodes
through the same `crates/tinker-pdf-filters/src/jbig2.rs` module that owns the
generic-region lineage, with clause 6.3's refinement beside it. What refuses,
refuses for a named reason (`Jbig2Refusal`), keeping the
placeholder-plus-warning contract of rulings 2 and 10 in
[../rulings.md](../rulings.md).

## Scope

- **Clause 6.5 symbol dictionary decoding**: the height-class loop, new-symbol
  bitmaps via the generic decoding procedure (6.5.8.1) with one shared context
  set per dictionary, import of symbols from referred-to dictionaries, and the
  export-flag runs of 6.5.10. Both the arithmetic and the Huffman variant
  (SDHUFF, including collective bitmaps coded with MMR).
- **Clause 6.4 text region decoding**: strip decoding with STRIPT and the S/T
  coordinate deltas, symbol ID codes, REFCORNER and TRANSPOSED placement,
  SBDSOFFSET, and the per-symbol combination operator, composited onto the page
  through `Bitmap::composite` (7.4.1.5 semantics).
- **Annex A integer arithmetic decoding** (IADH, IADW, IAEX, IAAI, IADT, IAFS,
  IADS, IAIT, IARI, IARDW/RDH/RDX/RDY) and the IAID procedure of A.3, layered on
  `MqDecoder::decode_at` / `MqContexts` in
  `crates/tinker-pdf-filters/src/mq.rs` — the coder is shared with JPX.
- **Annex B Huffman tables**: the fifteen standard tables B.1–B.15, custom
  tables from Tables segments (type 53, clause 7.4.13) resolved through segment
  references, and the runcode-assigned symbol ID code lengths of 7.4.3.1.7.
- **Clause 6.3 generic refinement region decoding** (GRTEMPLATE 0 and 1,
  TPGRON): it serves refinement-aggregate symbol coding (6.5.8.2), SBREFINE in
  text regions, and segment types 40/42/43.
- **Retained bitmap-coding contexts** (7.4.2): a dictionary that consumes a
  context starts from the one the *last* segment it refers to retained.
- **Segment references**: `Segment` keeps its number and its referred-to list
  — bounded, since the referred bytes must already fit inside the header —
  because a text region's symbol list is the concatenation of its referred-to
  dictionaries' exports in reference order (7.4.3), and custom tables are
  reached the same way. Shared dictionaries in `/JBIG2Globals` (ISO 32000-1
  7.4.7) need no plumbing of their own: globals segments are enumerated ahead
  of the image's own in `decode`.
- **Bounds** in the `crates/tinker-pdf/tests/bounds_ledger.rs` style: symbol
  count, total dictionary pixel budget, text-instance count, and one symbol's
  extent against its page, each a named constant checked before allocation (the
  `packed_size` pattern) and each a ledger row. See "Bounds" below.

## Non-goals

- **Halftone regions and pattern dictionaries** (clauses 6.6, 6.7; segment
  types 16, 20, 22, 23) are a third lineage and not this document's design.
  They decode, and `every_halftone_file_reproduces_the_generic_picture` in
  `crates/tinker-pdf/tests/jbig2_lineages.rs` holds them to the corpus picture
  across every grey-scale template, a sheared grid, four cell operators,
  `HENABLESKIP`, ten bitplanes and an intermediate region refined by a type 42
  segment. One clause fact matters there: on C.5's MMR road every plane rides
  one datastream ended by T.6's EOFB, and 6.2.6 byte aligns after it, so
  without the alignment the second plane would start one bit late.
- **Colour palette segments** (type 54) and the T.88 amendment features
  (EXTTEMPLATE, colour extension). A colour palette segment refuses by name
  (`UnhandledSegmentType`). [ROADMAP](../ROADMAP.md) row FI-16.
- **The random-access file organisation** (Annex D.1), refused in `segments()`
  as `RandomAccessOrganisation`. [ROADMAP](../ROADMAP.md) row FI-16.
- **Encoding a symbol dictionary, text region or refinement.** Only the generic
  region is written (see "The generic-region encoder" below). An encoder for
  this lineage could only be held to a round trip against this file's own
  decoder, and Annex H.1 publishes no symbol bitmaps to adjudicate it with.
  Lossless symbol and text-region encoding is [ROADMAP](../ROADMAP.md) row
  ED-27, and lossy symbol coding for scans OC-12; the adjudication question is
  ED-27's design to answer.
- **No new crate.** `jbig2_decode` is bytes-in/values-out per ruling 8, and the
  fuzz target's contract — success returns exactly the page the caller sized,
  failure is exactly `FilterError::Unsupported(Capability::Jbig2)` — holds for
  this lineage as for the generic one.

## Design

**Where it lives.** Everything is in `jbig2.rs` behind its seams.
`understood()` covers every segment type the `kind` module names except the
colour palette (type 54), which `carries_content()` names so that it refuses
rather than being skipped. `Page` holds a symbol store — a `BTreeMap<u32, Vec<Bitmap>>` keyed by
segment number, `BTreeMap` for deterministic iteration per ruling 4 — filled
by `kind::SYMBOL_DICTIONARY` and read by the text-region arm of the
dispatcher. A text region whose referred-to dictionary is missing or refused
draws nothing and does not count a region, so the `page.regions == 0`
refusal at the end of `decode` keeps catching whole-file failure; a *partial*
page (one region of several missing) degrades with the warning, the
generic-region truncation bargain.

The entry points are `jbig2_decode`, which reports the coarse `Warning`;
`jbig2_decode_attributed`, which reports each `Jbig2Refusal` instead; and
`jbig2_decode_measured`, which also returns a `Jbig2SymbolExtent`, the symbol
figures a census reads.

**Reuse, named.** Per-symbol bitmaps decode through `decode_arithmetic` with
the dictionary's own `MqContexts` living across symbols (6.5.8.1 requires the
shared adaptive state). The Huffman variant's collective bitmaps reuse
`crate::T6Rows` exactly as `decode_mmr` does — one T.6 implementation in the
crate, and the 1-black polarity agrees. Placement math accumulates CURS/CURT
in `i64` and clips through `Bitmap::composite`'s silent-clip contract, so no
coordinate on the wire can index out of the page.

**Placement.** 6.4.5 advances the running coordinate past the symbol *before*
drawing for two of the four reference corners and *after* it for the other
two, and which two depends on `TRANSPOSED` — the right-hand pair when the
strip runs across the region, the bottom pair when it runs down. Either way
the edge the coordinate named on entry is where that side of the symbol
lands, so the coordinate the strip advances is corner-independent in both
modes and only the *other* one branches. One `composite_signed` serves both.
`every_transposed_text_region_reproduces_the_ordinary_picture`
(`jbig2_lineages.rs`) holds the four transposed variants to the corpus
picture.

**Refusals** (rulings 2, 10). `Jbig2Refusal` holds the precise conditions, in
the shape `jpx::Refusal` and `JxrRefusal` use, and `Jbig2Refusal::warning`
maps each onto the closed `Warning` set:

| Warning | What it covers |
| --- | --- |
| `Jbig2SegmentSkipped` | A segment this build does not decode (the colour palette, the random-access organisation, an unknown data length it cannot resolve), data that is not what it claims to be, or a callee's refusal |
| `Jbig2VariantSkipped` | A reference that does not resolve — a missing or wrong-shaped retained context, a custom table the segment does not refer to, a text region whose dictionary is absent or refused — and a reserved table selector or refinement with no refinement tables |
| `Jbig2SymbolLimitHit` | A cap, or symbol data that contradicts itself or its header |
| `Jbig2RegionTooLarge` | A region or page above the caller's output ceiling |
| `Jbig2BudgetSpent` | The whole decode spent `MAX_JBIG2_WORK_MULTIPLE` times what the ceiling holds |

`Jbig2VariantSkipped` stays distinct from `Jbig2SegmentSkipped` so the residual
after a capability is named rather than folded into the refusal it replaced.
`Jbig2Refusal::is_malformed` separates a file that is broken from a capability
this build lacks. Refusals are asserted by name in `jbig2.rs`'s tests, the
`jpx::tests::refusals` pattern.

**Verification** ([../verification.md](../verification.md)).
`fuzz/fuzz_targets/jbig2.rs` asserts exact output size, the exact refusal
error, warning dedup, and swapped globals; its committed seeds in
`fuzz/corpus/jbig2/` are replayed on stable by
`crates/tinker-pdf-filters/tests/jbig2_seeds.rs`. Ruling 13 rules out asking
another decoder what a page should look like, so the anchors are the standard
itself (T.88 Annex H.1) and the corpus's own ground truth, below.
Determinism: integer-only decoding, no `HashMap`, and a JBIG2-bearing render
fingerprint in `crates/tinker-pdf/tests/determinism.rs`.

`crates/tinker-pdf/tests/jbig2_census.rs` is an `#[ignore]`d test that walks
every JBIG2 stream in the fetched corpora and tallies the flags — with a
segment-header reader **written from clause 7.2 and sharing nothing with
`jbig2.rs`**, because a census taken with the decoder's own reader would agree
with that reader by construction, including wherever it is wrong.
`crates/tinker-pdf/tests/jbig2_attribution.rs` answers the other question,
which only the decoder can: why this build refused a file, by
`Jbig2Refusal`. Both run in CI's corpus jobs.

### What the evidence can adjudicate

**Annex H.1.** The `ANNEX_H` constant in `jbig2.rs`'s tests is the whole T.88
Annex H.1 datastream, byte for byte, and its generic-region picture is
transcribed beside it. Its symbol bitmaps are not published, and cannot be
invented. What it does pin:

- **The dictionary, both ways.** Segment 2 carries two symbols with
  `SDHUFF = 1`, through height classes, a collective bitmap and Tables B.1,
  B.2 and B.4; segment 9 carries the same two with `SDHUFF = 0`, through the
  MQ coder and Annex A's integer decoders. The two share no code below
  `Segment`, and `annex_h_codes_the_same_symbols_two_ways` asserts they decode
  to **byte-identical** glyphs. A prefix length wrong by one bit desynchronises
  the reader and yields noise, not a glyph.
- **The generic region, both ways.** Pages 1 and 2 draw *different text* from
  their symbols, so `annex_h_codes_one_picture_twice_and_both_ways_agree`
  asserts what is true: the generic region is coded both ways and both match
  the published picture.
- **The numbering across references.** Page 2's arithmetic text region
  (segment 10) refers to segments 0 and 9, and segment 0 is page 1's Huffman
  dictionary. 7.4.3 numbers a text region's symbols across the concatenation
  of every dictionary it refers to, in reference order, so the arithmetic page
  depends on the Huffman variant. A region whose referred-to dictionary is
  absent or refused is refused *whole* (`DanglingReference`), because drawing
  it renumbered produces a page that looks like text and says something else.
- **Refinement, through both templates.** Annex H.1 carries no refinement
  region segment (types 40, 42, 43), but page 3 is a refinement fixture in
  the other two shapes: segment 17 is a symbol dictionary with `SDREFAGG=1`
  importing from segment 16, 6.5.8.2's refinement/aggregate symbol coding at
  `GRTEMPLATE=0`; segment 18 is a text region with `SBREFINE=1` at
  `GRTEMPLATE=1`. `annex_h_page_3_decodes_its_refined_text` holds it.

**A round trip proves plumbing, not convention.** An encoder in this
repository and a decoder in this repository agree by construction, so a round
trip says a choice is *load-bearing*, never that it is T.88's.
`a_text_region_places_its_symbols_where_6_4_5_computes` shows the strip
coordinate accumulating, the out-of-band value ending a strip rather than the
region, the gap being measured from the previous symbol's far edge rather than
its origin, and the symbol code being as wide as the count needs; it cannot
show that the placement convention itself is right.

**The corpus holds the twin the annex does not.** The pdf.js fixtures are one
399 by 400 bitmap encoded a dozen ways — four generic templates, MMR, TPGDON,
custom AT, a symbol dictionary, and refinement variants — each a lossless
encoding of the same picture. **The files that do not use a tool are
therefore ground truth for the files that do**, and the comparison is between
two of this engine's own decodes, so ruling 13 is untouched.
`crates/tinker-pdf/tests/jbig2_refinement.rs` points that instrument at clause
6.3 and `jbig2_lineages.rs` at the placement variants, custom tables, the
Annex B tables, retained contexts, unknown-length segments and halftones. They
are documents nobody here authored, and the instrument they are for is a whole
picture: an adaptive context is a probability model, so a decoder that gets it
wrong reads the same bits to a different answer and no unit test can see it.

**A fixture that passes under most values of an unknown is not evidence about
that unknown.** `bitmap-refine-customat-tpgron.pdf` reproduces under most
TPGRON slots, because its typical rows never fire. Likewise Annex H page 3's
dictionary refines a single 6 by 6 symbol — thirty-six coded decisions — which
is not enough information to separate candidate refinement templates; the
whole-picture fixtures are.

### Refinement (clause 6.3)

**The bit order in 6.3.5.3's figures is unobservable.** A context index names
an adaptive state slot and nothing else: the decoder reads and writes
`state[cx]`, every slot begins in the same state, and the MQ coder's A and C
registers are global. **Relabel every context through any bijection and the
decision sequence is bit-for-bit unchanged**, so only the *set* of positions
has to be right. `a_relabelled_refinement_template_decodes_identically` is that
argument as a test: the same thirteen positions in two different orders over
the same bytes, asserted to produce the same bitmap.

**The sets match the standard.** T.88's Figures 12 and 13 match the decoder
position for position:

| | figure | this file |
| --- | --- | --- |
| Template 0, destination | `RA₁` at (−1, −1), then (0, −1), (1, −1), (−1, 0) | `NOMINAL_REFINE_AT[0]` + `REFINE_0_HERE` |
| Template 0, reference | `RA₂` at (−1, −1), then the rest of the 3×3 | `NOMINAL_REFINE_AT[1]` + `REFINE_0_THERE` |
| Template 1, destination | (−1, −1), (0, −1), (1, −1), (−1, 0) | `REFINE_1_HERE` |
| Template 1, reference | a plus: (0, −1), (−1, 0), (0, 0), (1, 0), (0, 1), (1, 1) | `REFINE_1_THERE` |

Thirteen pixels and ten, which is what the two figures are captioned. 6.3.5.2's
edge rule — "all pixels lying outside the bounds of the actual bitmap or the
reference bitmap have the value 0" — is `Bitmap::get`'s behaviour, and the
alignment is `(X − GRREFERENCEDX, Y − GRREFERENCEDY)`, which is the *minus*
`decode_refinement_into` uses.

**TPGRON is the one number the relabelling does not cover.** 6.3.5.6's
pseudo-context is a bare number in the standard's own ordering, so in this
file's bit order it is determined from the corpus rather than transcribed: of
the 8 192 values template 0's slot can take, exactly one reproduces
`bitmap-refine-tpgron.pdf`, and of the 1 024 for template 1, exactly one
reproduces `bitmap-refine-template1-tpgron.pdf`. Those are `TPGRON_0` and
`TPGRON_1`.

**Clause facts the decoder depends on:**

- 6.5.8.2.3 makes the symbol code as wide as the whole dictionary needs —
  `SDNUMINSYMS + SDNUMNEWSYMS` — not as wide as the symbols decoded so far.
- 6.5.8.1: a dictionary carries one set of adaptive states from one symbol to
  the next, and that is as true of refinement as of the generic procedure. The
  *coder* restarts at each byte-aligned sub-stream because the stream does,
  but the statistics do not.
- 7.4.6.1: an intermediate region is not drawn. It waits in an auxiliary
  buffer for the segment that refers to it, so an intermediate text region is
  not composited, and an intermediate generic region is decoded and kept for a
  refinement region to refine.
- **The two reference offsets differ.** 6.4.11 *centres* a refined bitmap on
  the instance it replaces and shares the size difference between the two
  edges; 6.5.8.2.2 builds a symbol out of another symbol, with no instance and
  no strip, and its offset is `(RDX, RDY)` as coded. `refinement_offset` is
  6.4.11's alone, on both the arithmetic and the Huffman dictionary roads, and
  both dictionary encoders in the test module place a refined symbol at
  `(RDX, RDY)`. What adjudicates the reading is a real scan:
  `safedocs/0000337.pdf` refines symbols into different shapes, and decodes
  with no warning only under `RDX` alone.
  `a_refined_symbol_that_is_not_its_reference_s_size_pins_6_5_8_2_2` refines
  one 6 by 6 symbol into an 8 by 6 and then a 5 by 7, both at
  `RDX = RDY = 0`, and is the guard that stops 6.4.11's arithmetic leaking
  back; `split_offset` in the test module is written out separately from
  `refinement_offset` so a defect in the decoder is not mirrored by the
  encoder.

**The symptom is not diagnostic.** A desynchronised arithmetic decoder produces
noise whatever pushed it out of step: a mis-sized symbol code, a wrong
template and adaptive states reset per symbol all present as "the refinement
context template must be wrong". That is the argument for keeping a fixture
that renders a whole page wherever a decoder has an internal state a header
cannot check.

### Annex B tables and Huffman refinement

**The tables are transcribed from ITU-T Rec. T.88 (02/2000)**, which is
published free of charge. Three things hold them to it:

- The standard prints an `Encoding` column beside every line: the bit string
  itself. That column is redundant with the prefix lengths if and only if
  B.3's canonical assignment is right, so it is an independent check on the
  assignment *and* a second reading of the same table;
  `the_codes_b3_assigns_are_the_ones_the_standard_prints` asserts the printed
  codes of B.7, B.10, B.12 and B.15.
- `annex_h_codes_the_same_symbols_two_ways`, above.
- `jbig2_lineages.rs`'s `ANNEX_B_TABLES` family, because a table that is
  complete and wrong does not refuse — it draws a different picture, and only
  a whole picture can say so. Completeness (the lengths summing to one) is
  necessary and not sufficient.

**One clause fact follows.** No standard `SBHUFFDT` table can code zero: B.11,
B.12 and B.13 all begin at 1. So 6.4.5 step 1's initial delta is at least one
and a Huffman text region's first strip begins at `-SBSTRIPS` or above.

**Huffman refinement.** 6.4.11's envelope — `RI` as one plain bit, the four
deltas through a selected table, `BMSIZE`, byte alignment, and an arithmetic
sub-stream the bit reader steps over — decodes for text regions, and the
dictionary road (`SDHUFF` with `SDREFAGG`) decodes through both 6.5.8.2.2's
single refinement and 6.5.8.2.1's aggregate. `bitmap-symbol-texthuffrefine.pdf`
selects B.14 and `bitmap-symbol-texthuffrefineB15.pdf` selects B.15.

**The corpus adjudicates the envelope, not the tables**: those files code every
delta as zero, so the only line either table exercises is its one-bit code for
0. Three fixtures in `jbig2.rs` code one picture two ways with non-zero
deltas:

- `a_huffman_text_region_refines_through_b14_with_non_zero_deltas` selects B.14
  with `(RDW, RDH, RDX, RDY) = (2, 1, −1, −2)` — all four of its three-bit
  lines — and compares the page against the same picture placed by an
  *arithmetic* dictionary and region, over two roads that share almost no
  code.
- `a_huffman_text_region_refines_through_b15_with_non_zero_deltas` does the
  same through B.15, whose lines carry an offset inside a range as well as a
  prefix.
- `a_huffman_dictionary_refines_at_every_non_zero_line_of_b15` walks B.15
  outwards — a one-bit range, then two, three and four either side of zero,
  then the two open-ended lines at ±31 — refining once per pair and comparing
  each against the same symbol coded plainly.

Both text-region fixtures place their instance at (2, 3) rather than at the
origin so the bit reader is **not** on a byte boundary when `BMSIZE` has been
read; 6.4.11's alignment before the sub-stream is then a fact they depend on.

What refuses: a reserved selector value, a custom-table selector the segment
refers to no Tables segment for (`CustomTableMissing` — 7.4.3.1.6 and
7.4.4.1.2 hand referred-to tables out by position, so one short renumbers
every selector after the gap), and a delta that sizes an instance to nothing
(`RefinedSizeOutOfRange`). `a_refining_huffman_region_still_refuses_what_it_cannot_read`
asserts the selector and size cases by name.

### Bounds

Four caps, each `pub`, each a row of `crates/tinker-pdf/tests/bounds_ledger.rs`
with a `**This cap**` table in its own doc comment and a clock-free `#[test]`
in `jbig2.rs` that fires it by name:

| | `MAX_JBIG2_SYMBOLS` | `MAX_JBIG2_SYMBOL_PIXELS` | `MAX_JBIG2_TEXT_INSTANCES` | `MAX_JBIG2_SYMBOL_PAGE_MULTIPLE` |
| --- | ---: | ---: | ---: | ---: |
| A 200-page bilevel scan sharing one global dictionary | 20 000 | 25 000 000 | 5 000 per region | 1 |
| A 300-page reflowable book | 0 | 0 | 0 | 0 |
| The cap | 100 000 | 67 108 864 | 4 194 304 | 4 |

The first three yardsticks are **arithmetic about a plausible file, written
down so it can be argued with**: a 300 dpi A4 text page reduces to a few
hundred distinct glyph bitmaps and a few thousand placements, so a 200-page
document sharing one global dictionary reaches the low tens of thousands of
symbols. Their published figures carry `(estimate)`, and
`every_bound_publishes_the_number_it_is` parses the suffix. The comic and
fixed-document columns hold that scan although neither format decodes JBIG2,
because the honest zero would leave the only two "does this cap refuse a real
file" checks saying nothing about these rows; `SCAN_SYMBOLS` carries that
notice.

The second cap is in pixels because that is what the decoder charges; a byte
figure would have to pick a packing. A symbol's *size* is nowhere in a segment
header — 6.5.5 accumulates both dimensions inside the arithmetic coder — so a
census that shares no code with the decoder can count symbols and cannot
measure one. `jbig2_decode_measured` is how it measures them.

**`MAX_JBIG2_SYMBOL_PAGE_MULTIPLE` is a relation rather than a quantity.** It
bounds one symbol against the page it will be composited onto, so its currency
is a multiple of `/Width` and `/Height`. T.88 does not forbid a symbol larger
than its page, so this is a bound this build chooses. Its yardstick is a
measurement rather than arithmetic: `jbig2_census.rs` decodes every symbol of
every JBIG2 image in the fetched corpora and reports the smallest whole
multiple of each page that admits its symbols. It is charged **per dimension**
rather than per area: a symbol one row tall and a page's worth of pixels wide
has the page's area and none of its shape, and clipping it to the page loses
all but one row; an area bound admits that symbol and this one does not. The
seed `fuzz/corpus/jbig2/symbol-dictionary-spends-the-pixel-budget` is that
symbol, 246 988 by 1 against a page of 1 by 1.

### The generic-region encoder

Not this document's lineage, but it shares the coder. `mq.rs`'s `MqEncoder` is
public, and `jbig2/encode.rs` (`jbig2_generic_encode`,
`jbig2_generic_region_segment`) walks the decoder's own row loop: the decision
sequence is the same one and the context function is literally the decoder's
`context()`. It takes a packed raster and a caller's stride rather than a
`Bitmap`, copying with the padding bits past the width cleared, and
`Jbig2EncodeError` refuses a caller that describes its own buffer or template
wrongly. Ruling 2 does not apply to any of them; there is nothing to degrade
to when the pixels asked for were never handed over.

**What it writes, and what it does not.** All four templates at any AT
positions, 6.2.5.7's TPGDON with the canonical SLTP choice, and 7.4.6's region
segment *data*. Not: MMR generic regions (`ccitt_g4_encode` is the T.6 coder
6.2.6 defers to, and a second copy is a second thing to get wrong), symbol
dictionaries, text regions, refinement, halftones, pattern dictionaries,
USESKIP, or any part of a JBIG2 file above one segment's data.

**What adjudicates it, and its limit.** Annex H.1 segment 11 — nine published
bytes for a published picture at template 0 with TPGDON and the nominal AT
pixels. Encoding the picture reproduces the bytes, which pins the MQ
registers, `BYTEOUT`, `FLUSH`, the AT layout, 6.2.5.7's SLTP rule and *which*
pixels template 0 reads. It does **not** pin the context numbering: a context
index is only a label into an array whose slots all start identical, so any
bijection of the numbering is invisible to a coder, and a published bitstream
is what a coder produced. A second published datastream would not fix that.
`template_context_bits_match_the_figures` is the test that holds the
numbering to the figures.

## Milestones

1. **Corpus census of the JBIG2 files.** `crates/tinker-pdf/tests/jbig2_census.rs`
   prints per-file SDHUFF/SBHUFF/SDREFAGG/SBREFINE/retention tallies.
2. **Segment references and Annex A integer decoders.** `Segment` carries its
   number and referred-to list; `every_integer_range_and_oob_round_trips`
   covers all six A.2 fields at both ends and OOB;
   `the_symbol_index_procedure_round_trips_at_every_code_length` covers A.3
   including `SBSYMCODELEN` zero; the committed fuzz seeds are replayed on
   stable by `tests/jbig2_seeds.rs`.
3. **Symbol dictionary, arithmetic.** Symbols round-trip pixel-for-pixel
   against an `MqEncoder`-built dictionary over three height classes and all
   four templates; 6.5.10's export runs select across imported and new
   symbols. Annex H.1 publishes no symbol pictures, so the pixel-for-pixel
   claim is against a fixture this repository builds rather than against the
   standard.
4. **Text region, arithmetic.**
   `a_text_region_places_its_symbols_where_6_4_5_computes` places symbols
   across two strips at the coordinates 6.4.5 computes;
   `a_text_region_whose_dictionary_refused_is_refused_by_name` holds a region
   whose referred-to dictionary is absent or refused to refusing **whole**;
   all four reference corners, SBDSOFFSET, multi-strip regions and TRANSPOSED.
5. **Clause 6.3 refinement, 6.5.8.2 aggregate, SBREFINE, segment types
   40/42/43.** Annex H page 3 decodes into its refined text with no warning;
   every corpus refinement variant reproduces the unrefined picture with 0
   pixels different; `a_relabelled_refinement_template_decodes_identically`
   pins the argument the templates rest on.
6. **Annex B Huffman: standard tables, type-53 custom tables, 6.5.9's
   collective bitmaps via `T6Rows`, 7.4.3.1.7's symbol-ID codes.**
   `annex_h_codes_the_same_symbols_two_ways`,
   `the_codes_b3_assigns_are_the_ones_the_standard_prints`, and
   `jbig2_lineages.rs`'s Annex B and custom-table families.
7. **Bounds and fuzz hardening.** The four caps are rows in `bounds_ledger.rs`
   and none refuses a real scan.
8. **Corpus closure and docs.** Every JBIG2-bearing corpus file renders without
   a placeholder warning; a JBIG2 fingerprint in `determinism.rs`; the
   symbol/text refusal rows leave [../features/filters.md](../features/filters.md).

## Dependencies

- `crates/tinker-pdf-filters/src/mq.rs` — `MqDecoder`, `MqContexts`, and
  `MqEncoder`; shared with JPX, and pinned by the Annex H.2 test.
- `crates/tinker-pdf-filters/src/jbig2.rs` — `Segment`, `Reader`, `Bitmap`,
  `RegionInfo`, `packed_size`, `Page`, the `ANNEX_H` fixture, and the
  `Warning`/`Capability` enums in `lib.rs`.
- `crate::T6Rows` (the T.6 decoder `decode_mmr` reuses) for Huffman-variant
  collective bitmaps.
- Corpus infrastructure: `cargo xtask corpus-fetch` / `corpus-run` and
  `corpus/ratchet.json`. Ruling 13 means the corpus run measures refusals and
  the absence of placeholder warnings rather than comparing pixels with
  anything; T.88 Annex H.1 and the corpus's own ground truth carry the
  pixel-exact weight instead.
- `crates/tinker-pdf/tests/bounds_ledger.rs` (ledger rows) and
  `crates/tinker-pdf/tests/determinism.rs` (fingerprint).
- `fuzz/fuzz_targets/jbig2.rs` and its committed seed corpus.

## Risks

| Risk | Mitigation |
|---|---|
| Context-lifetime bugs (dictionary contexts shared across symbols, IAID width derived from symbol count) decode plausibly wrong pages rather than crashing | Annex H.1 pixel-exact assertions in both coding variants — the standard's own bitmap — and the corpus's own ground truth: `jbig2_refinement.rs` and `jbig2_lineages.rs` hold each coding variant to the plain generic-region decode of the same picture. Beyond those, the corpus can say a file decoded, not that it decoded correctly |
| Symbol dictionaries invite allocation blowup: 32-bit symbol counts, per-symbol bitmaps, per-strip instance counts | Every count capped by a named budget checked before allocation (`packed_size` pattern); budgets are ledger rows set against a 200-page scan, so no cap refuses the thing the format is for |
| Dangling references: text region whose dictionary is in a missing globals stream, or refused over budget | Region refused whole and not counted; a page with zero regions refuses by name; the fuzz target swaps globals and own streams every run |
| Signed placement arithmetic (S/T deltas, REFCORNER, TRANSPOSED) indexing off-page | `i64` accumulation, clipping only through `Bitmap::composite`'s bounds-checked path; fuzz asserts exact output size on every success |
| A coding-variant refusal that reads like a lineage nobody started | `Jbig2VariantSkipped` is a distinct warning from `Jbig2SegmentSkipped`, and the refusals beneath both are asserted by name, so the residual is named rather than folded in |
