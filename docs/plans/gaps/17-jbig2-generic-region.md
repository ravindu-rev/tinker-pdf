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

## Amendment — August 2026: the corpus evidence ruling 3 asked for

[23](23-corpus-runner.md) has run. Across 4 525 documents from pdf.js,
veraPDF, qpdf's qtest and the PDF Association:

**JBIG2: 103 files, 2.3 % — the highest hit rate of the three deferred
capabilities**, against JPX at 0.4 % and mesh shadings at 0.2 %.

So of the three plans ruling 3 defers to corpus evidence, this is the one the
evidence most supports, and by an order of magnitude over JPX.

Two things temper it, and both belong in the record rather than in a footnote.
**Every one of the 103 is in `pdfjs`** — a browser's regression suite, weighted
towards files that once broke a browser. `verapdf` (2 907 files, a conformance
corpus) and `qpdf` (637, a writer's test suite) contain **no JBIG2 at all**. A
2.3 % rate drawn entirely from one corpus of known-awkward documents is not the
same claim as 2.3 % of documents in the world. And scanned-document corpora,
which is where JBIG2 actually lives, are not represented here at all — so this
number is as likely to be an under-count for real-world archival material as an
over-count.

The other prerequisite is also discharged: [16](16-ccitt-completion.md) landed
and left `T6Rows::new(data, bit_offset, columns)` for this plan's MMR path — see
the amendment above.

## As built — 15 August 2026

Milestones 1 to 7, all seven. The generic-region lineage decodes and
everything else is refused by name.

### The MQ module's public shape, for gap 18

`crates/tinker-pdf-filters/src/mq.rs`, its own module from the first commit
because T.88 Annex E and T.800 Annex C are the same coder and
[18](18-jpx-decision.md) says the decoder "moves to a shared module" —
starting it anywhere else would have opened that plan with a refactor. It is
`pub mod mq`, re-exported from the crate root as `MqContext`, `MqContexts`
and `MqDecoder`:

```rust
pub struct MqContext;                     // Clone + Copy + Debug + Default + PartialEq + Eq
pub struct MqContexts { /* ... */ }
impl MqContexts {
    pub fn new(len: usize) -> MqContexts; // every context at E.3.6's initial state
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn reset(&mut self);              // between regions, without reallocating
}
pub struct MqDecoder<'a> { /* ... */ }
impl<'a> MqDecoder<'a> {
    pub fn new(data: &'a [u8]) -> MqDecoder<'a>;        // INITDEC (E.3.5)
    pub fn position(&self) -> usize;                    // saturating; for a caller that parses on
    pub fn decode(&mut self, cx: &mut MqContext) -> u8; // DECODE (E.3.2)
    pub fn decode_at(&mut self, cx: &mut MqContexts, index: usize) -> u8;
}
```

It follows Annex E's formulation rather than Annex G's software conventions:
`C` is a 32-bit register whose top sixteen bits are the `Chigh` the figures
compare against `Qe`, so the code reads next to E.3.2's flowcharts instead of
next to a transformation of them. JPEG 2000 would want nineteen contexts where
JBIG2 wants up to 65 536, and nothing in the module knows which — context
*numbering* is the caller's business, which is the property that lets the two
codecs share it.

There is a test-only `mq::encoder::MqEncoder` beside it (E.3.7, E.3.8). It is
not part of the shipped surface; it exists so that a generic-region round-trip
means something, and it is itself pinned by re-encoding Annex H.2's 256
published decisions back into Annex H.2's thirty published bytes.

Two ruling-1 edges are closed by construction rather than by a check. Reading
past the end of the data yields `0xFF`, which is E.3.4's marker convention, so
`BYTEIN` stops advancing and a truncated stream terminates rather than running
off the buffer. `RENORMD` shifts a register that is provably non-zero, so it
converges within fifteen turns, and carries a hard bound anyway for the state
that cannot happen.

### What the refusal covers

`jbig2_decode` returns `Err(FilterError::Unsupported(Capability::Jbig2))`
whenever **no region was composited onto the page**, and `resources.rs` turns
that into the neutral placeholder and a named codec. That covers, by name:
symbol dictionaries and text regions (6.4, 6.5); pattern dictionaries and
halftone regions; refinement regions; an *intermediate* generic region, which
decodes perfectly well but which 7.4.6.1 sends to an auxiliary buffer for a
refinement segment to refer to rather than onto the page; a region larger than
the output ceiling; an MMR region that decodes no row at all; the
random-access organisation, which a PDF cannot carry and which would invent
segments if it were parsed as sequential; and a segment carrying the
unknown-length sentinel, after which nothing else can be located.

It also covers the case the plan warned about most, and in both directions. A
file with a generic region *and* an undecodable symbol dictionary **draws the
region** and reports the missing lineage — refusing it would throw away a
picture that decoded perfectly. A file with no decodable region is refused.
Both are tested at the crate boundary and again through a rendered page.

### What proportion of real JBIG2 this decodes versus refuses

**The minority of files, and it matters that this is written down rather than
inferred from "JBIG2 works now".**

Gap 23 measured JBIG2 at 103 files, 2.3 % of 4 525 — the highest hit rate of
the three capabilities ruling 3 defers, and an order of magnitude above JPX.
What it did not measure is the *lineage* split inside those files, and no run
here has measured it either, so what follows is a claim about producers rather
than a count.

The generic-region lineage is what a scanner or a fax-to-PDF path emits: a
whole-page bilevel image, arithmetic or MMR, with no dictionary. The symbol
dictionary plus text region lineage is what `jbig2enc` and OCRmyPDF emit, and
an OCR pipeline is the overwhelmingly common reason a JBIG2 stream is inside a
PDF at all — the format's whole selling point for scanned text is that it
factors repeated glyphs into a dictionary, and the producers that matter all
take it. So the expectation is that this decodes the **minority** of real
JBIG2 by file count, plausibly well under half, and refuses the rest.

That is the outcome the plan chose knowingly rather than a disappointment: the
refused half is roughly 2 500 further lines with the integer arithmetic
decoders and the standard Huffman tables, and the scope section calls it a
separate decision. What this gap guarantees is that the refusal is *correct* —
a refused file draws the placeholder it drew before, never a blank page — so
the engine is strictly better off than it was and never worse.

### Three things the plan does not describe

**It is Annex H.1, not H.2.** Milestone 3 asks for "H.2's generic-region
datastream". H.2 is the arithmetic coder test sequence, which milestone 1
already used; the datastream example is **H.1**. It is transcribed whole — 860
bytes, twenty-one segments, three pages — and it is worth far more than one
picture, because its first two pages are the *same image coded two different
ways*: page 1 with MMR, page 2 arithmetically on template 0 with typical
prediction. Two decoders that share no code have to agree pixel for pixel, and
they do. No round-trip against an encoder written here could have said that.

**A context bit order is unobservable from any datastream.** Relabelling the
context bits is a bijection on the context array; every adaptive state starts
identical, so an encoder's slot histories and a decoder's stay in step under
any permutation. Transposing two of template 0's bits was injected, and Annex
H.1 still decoded to its published picture byte for byte with every round-trip
still passing. The same goes for moving typical prediction's pseudo-context to
an unused neighbour: `0x9B24` decodes Annex H.1 perfectly.

Both are real bugs — 6.2.5.7's pseudo-context is a *literal* slot number, so
the moment typical prediction is on, the numbering stops being a free choice
and becomes part of what the encoder agreed to — and neither is reachable by
any fixture that could be written. The defence is a transcription of Figures 8
to 11 that sets one pixel at a time and demands the bit the figure assigns it,
plus the four pseudo-contexts asserted against the standard directly. That one
test catches three injected defects that nothing else in the repository
catches.

**An intermediate generic region moved from "understood" to "refused".** It
was in the decodable set when milestone 2 landed. 7.4.6.1 makes an
intermediate result an auxiliary buffer for a later segment to refer to, and
the only thing that refers to one is a refinement region, which this build
refuses — so compositing it would draw a working buffer as finished content.

### Polarity, and where it lives

T.88 6.2.2 codes 1 for black; a 1-bit DeviceGray sample is 0 for black. The
decoder returns JBIG2's own sense unconverted, exactly as `T6Rows` already
did, and `jbig2_samples` in `crates/tinker-pdf/src/resources.rs` inverts once
— in the same function that reads `/Decode`, `/ImageMask` and `/ColorSpace`,
so one place in the build knows which convention it is translating between.

Output **joins the generic sample path**, which is the same decision gap 16
made for a fax and for the same reason: an image that hands back its own
pixels has to reimplement those three keys to compose at all, and a scanned
page is an `/ImageMask` about as often as it is a DeviceGray image. `/Decode
[1 0]` inverting a scan and `/ImageMask true` painting one in the fill colour
are each a test, and a build that returned RGB would fail both while passing
everything else.

### Evidence

Twenty-one tests in `jbig2.rs`, seven more in `mq.rs`, eight at the render
boundary in `crates/tinker-pdf/tests/jbig2.rs`, and the repository's tenth
determinism fingerprint — which `wasm32-wasip1` under wasmtime reproduces
byte-for-byte against native Windows, so a 32-bit target and a 64-bit one
agree about a JBIG2 decode. **None of the other nine moved.**

Sixteen streams from the JBIG2 test corpus SerenityOS publishes — every
template, each with and without custom AT pixels and typical prediction, plus
the MMR one — decode to that corpus's own 399 by 400 reference bitmap exactly.
They are not committed, because they are somebody else's files rather than the
standard's; they are recorded here because they are what says the context
numbering agrees with what the rest of the world encodes against, which
nothing written inside this repository could establish.

The `jbig2` fuzz target ran for ten minutes on four jobs: **1 664 926
executions, no crash, no out-of-memory, no timeout**, corpus grown from 22
committed seeds to 1 319 inputs. A coverage replay over a *copy* of that
corpus puts jbig2.rs at 97.8 % of regions and 98.5 % of lines with **all 36
functions executed**, and `decode_arithmetic`, `context`, `generic_region`,
`tpgdon_context` and `template_bits` each at 100 % of both — so all four
templates and all four pseudo-contexts were reached by real inputs rather than
by assertion.
