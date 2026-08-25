# JBIG2 symbol dictionary and text region

When this is done, the symbol-dictionary-plus-text-region lineage of ITU-T T.88 —
what `jbig2enc` and OCR pipelines emit, and therefore most JBIG2 in circulation —
decodes through the same `crates/tinker-pdf-filters/src/jbig2.rs` module that
already owns the generic-region lineage, and the engine's highest-reachability
refusal (103 files in the pdf.js corpus, per [../ROADMAP.md](../ROADMAP.md) Tier 2)
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

**So refinement comes before Huffman, and milestones 5 and 6 are swapped from
the order this document first proposed.** The expectation was
arithmetic ≫ Huffman ≫ refinement, on the grounds that `jbig2enc` emits only
arithmetic coding; the corpus is not all `jbig2enc`, and refinement unlocks
nearly twice what Huffman does. That is what milestone 1 was for.

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
| 3 | Symbol dictionary, arithmetic, SDREFAGG=0 | `ANNEX_H`'s arithmetic symbol dictionary decodes; exported symbol count and bitmaps assert pixel-for-pixel against the annex; import-from-referred-dictionary and export-flag runs unit-tested | M |
| 4 | Text region, arithmetic (SBREFINE refused by name) | `ANNEX_H` page with arithmetic text region matches the published page bitmap in its window; `a_symbol_dictionary_file_refuses_and_says_so` flips to a success assertion; `Jbig2RefinementSkipped` reachability test passes; seed-writer test emits the new fixtures | M |
| 5 | Clause 6.3 refinement + 6.5.8.2 aggregate + SBREFINE + segment types 40/42/43 — **ahead of Huffman, by milestone 1's census: 9 files against 5** | `MqEncoder`-built refinement fixtures decode; `Jbig2RefinementSkipped` reachability test deleted with the closure; injected wrong-template defect caught by a counted assertion | M |
| 6 | Huffman variants: Annex B tables, type-53 custom tables, 7.4.3.1.7 symbol IDs, MMR collective bitmaps via `T6Rows` | H.1's Huffman-coded page decodes pixel-identical to its arithmetic twin; an over-subscribed custom table refuses with an asserted warning | M |
| 7 | Bounds and fuzz hardening | `MAX_JBIG2_SYMBOLS`, `MAX_JBIG2_SYMBOL_BYTES`, `MAX_JBIG2_TEXT_INSTANCES` rows in `bounds_ledger.rs`, each measured against a real `jbig2enc`/OCRmyPDF output and none refusing it; a recorded fuzz session over the extended seeds with zero crashes | S |
| 8 | Corpus closure and docs | `cargo xtask corpus-run` shows `Capability::Jbig2` hit-rate ~0 in `ratchet.json`; every JBIG2-bearing corpus file renders without a placeholder warning, counted; one JBIG2 fingerprint in `determinism.rs`; the symbol/text refusal rows leave [../features/filters.md](../features/filters.md) | S |

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
