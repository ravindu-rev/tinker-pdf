# JBIG2 is refused

JBIG2 is how scanners and OCR pipelines compress bilevel page images — a
scanned archive is often entirely JBIG2. The engine reports it and draws a
grey placeholder. When this is done, the generic-region lineage decodes and
everything else is *refused by name* rather than silently returning a blank
page. (L)

## What is wrong

Nothing is subtly wrong. `Capability::Jbig2` exists, `ImageCodec::capability`
returns it, the chain terminates with `EncodedImage`, and the render path
draws a placeholder with a named warning. The degradation is correct.

Two things make this worth planning carefully rather than just building.

**Ruling 3 schedules it by evidence**, and the evidence does not exist — no
corpus has been run ([23](23-corpus-runner.md)). Generic region is small
enough that building it on judgement is defensible; the rest is not.

**The lineage split decides the value.** Generic region covers scanner output.
Files from `jbig2enc` and OCRmyPDF — which is most PDFs that have been through
an OCR pipeline — are symbol dictionary plus text region, a different and much
larger body of work.

## Scope

- The MQ arithmetic decoder (T.88 Annex E), including the 47-row Qe table.
- Generic region decoding (6.2): templates 0–3, AT pixels, TPGDON with its
  pseudo-contexts.
- MMR-coded generic regions (6.2.6), reusing the T.6 decoder.
- Segment headers (7.2) and the embedded-stream organisation PDF uses
  (Annex D.3), including `/JBIG2Globals`.
- Page information segments, and the region composition operators.
- **A refusal when no region was decoded** — see below.

## Non-goals

- **Symbol dictionaries and text regions** (6.4, 6.5, 7.4.3, 7.4.4). Roughly
  2,500 further lines with the integer arithmetic decoders and the standard
  Huffman tables. A separate decision once the corpus says how much of the
  real world needs it.
- **Halftone regions and refinement.** Rare.

## Design

**The MQ decoder is shared with JPX** if [18](18-jpx-decision.md) ever
proceeds — T.88 Annex E and T.800 Annex C are the same coder. Put it in its
own module and pin it with T.88's Annex H.2 test sequence as a permanent test,
not a scaffolding one, because a change made for one codec would otherwise
silently alter the other.

**Allocation before decoding.** Region width and height are attacker-controlled
32-bit values, and a 300 dpi A4 page at one byte per pixel is 8.7 MB. Every
allocation goes through a checked multiply against the output ceiling
*before* it happens — the pattern the CCITT decoder already uses.

**Polarity.** JBIG2 has 1 = black; a 1-bit DeviceGray image has 0 = black.
State the inversion at the boundary rather than burying it.

## Where a half-implementation is worse than none

**Skipping unknown segments and returning the page.** A file that is symbol
dictionary plus text region would then decode its page information segment,
find no generic region, and return a **blank white page reported as success** —
indistinguishable from a correct decode of a blank scan, and strictly worse
than today's grey placeholder, which at least says something is missing.

The refusal path is not polish. It is the feature. If no region was decoded,
return the capability error and let the placeholder stand.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | MQ decoder | T.88 Annex H.2's test sequence reproduces exactly — transcribed into the repository, since no JBIG2 test material exists anywhere in the tree | S |
| 2 | Segment headers, embedded organisation, `/JBIG2Globals` | A hand-built stream's segments enumerate correctly; an unknown segment type is skipped and *recorded* | S |
| 3 | Generic region, template 0, no TPGDON | H.2's generic-region datastream decodes to its published bitmap | M |
| 4 | Templates 1–3, AT pixels, TPGDON | Each template round-trips a hand-built region; TPGDON's pseudo-contexts are exercised | M |
| 5 | MMR generic regions | An MMR region decodes through the existing T.6 path | S |
| 6 | The refusal | A symbol-dictionary file returns the capability error and draws the placeholder — **never** a blank page reported as success | S |
| 7 | Fuzz and limits | `cargo fuzz run jbig2` survives a session; a region declaring 2³² pixels is refused before allocating | S |

## Dependencies

**Needs first:** [23](23-corpus-runner.md) for the evidence that schedules it
(ruling 3), and [16](16-ccitt-completion.md) for the T.6 decoder the MMR path
reuses.

**Unblocks:** scanned-archive corpora, which are otherwise entirely
placeholder.

## Risks

| Risk | Mitigation |
| --- | --- |
| No JBIG2 test material exists in the tree — `testdata/` is four self-authored PDFs | Transcribe T.88 Annex H.2 by hand; it is a few hundred bytes and is the highest-value single artefact in this plan |
| The shared MQ decoder is changed for JPX and silently breaks JBIG2 | H.2 is a permanent test, not deleted once generic region works |
| Adding `Warning` variants is a public API change with exhaustive match sites | The compiler catches the two in the filters crate; check whether any consumer maps `Warning` with a wildcard arm, which would swallow the new ones |
| Flipping the capability changes what the capability surface means, which plan 02 milestone 4 pins | Update the plan file and the assertion in the same commit, deliberately, rather than editing the test to make CI green |

## Amendment — August 2026: the T.6 seam exists, use it

This plan's MMR path says it reuses "the existing T.6 decoder", and names
[16](16-ccitt-completion.md) as the prerequisite. At the time that was
aspirational: `crates/tinker-pdf-filters/src/ccitt.rs` exported only a
whole-stream `decode(data, params, max_output)` starting at bit zero, which an
MMR region embedded in a larger segment cannot use. Nothing in either plan
asked anyone to build the seam, so this plan would have duplicated T.6.

Gap 16 built it. **Use this rather than writing a second one:**

```rust
pub struct T6Rows<'a> { /* ... */ }

impl<'a> T6Rows<'a> {
    pub fn new(data: &'a [u8], bit_offset: usize, columns: u32) -> T6Rows<'a>;
    pub fn next_row(&mut self, row: &mut [u8]) -> bool;
    pub fn row_bytes(&self) -> usize;
    pub fn bit_position(&self) -> usize;
}
```

It starts wherever the caller says, decodes against an imaginary all-white
reference line (T.6 2.2.1), writes packed 1-bpp rows most significant bit
first, takes any buffer at least `row_bytes()` long, and reports where it
stopped so the caller can carry on with the segment. It shares `decode_row`
with the whole-stream entry point, so a change to the mode codes must keep both
correct — and the `ccitt` fuzz target now drives both, asserting that a row
cannot decode out of no bits and that nothing is written past a row's own
width.

Two other things from gap 16 that bear on this plan:

- Output is packed 1-bpp everywhere now, which is the shape a JBIG2 region
  wants anyway. There is no byte-per-pixel conversion left to undo.
- `ccitt_samples(...)` in `crates/tinker-pdf/src/resources.rs` is the single
  entry point for decoded fax samples. If a JBIG2 region ever needs to reach
  the generic sample path, that is the shape to follow.

Unchanged from the original plan, and still the instruction: put the MQ
arithmetic decoder in **its own module from the first commit**, with T.88
Annex H.2 as a permanent test rather than scaffolding. [18](18-jpx-decision.md)
says the MQ decoder "moves to a shared module"; if it starts inside `jbig2.rs`
that plan begins with a refactor inside an already-large commit.
