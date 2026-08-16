# JPX, option A: the decoder, and the three questions it must not start without

JPEG 2000 in PDF. The engine reports it and draws a placeholder. When this is
done, a JPX image decodes to pixels, and every codestream this build cannot
decode correctly is **refused by name** rather than turned into a plausible
photograph of nothing. (XL)

This is the plan [18](18-jpx-decision.md) says has to exist before option A
can be handed to anybody: *"Only for option B, since A needs its own plan
written after the decision."* Gap 18's milestone table is option B's — a JP2
box walk, colour-space inference, honest reporting — and those three
milestones describe different work from the one below. This document replaces
them for option A and leaves gap 18 as the decision record it is.

## Why this is being built, said plainly

**By decision, against the evidence, and against gap 18's own amended
recommendation.**

[23](23-corpus-runner.md) ran across 4 525 documents from pdf.js, veraPDF,
qpdf's qtest and the PDF Association. **JPX: 19 files, 0.4 per cent.** JBIG2
is 2.3 per cent over the same corpus and mesh shadings are 0.2 per cent. Gap
18's August amendment reads that number and argues for option B — the 150-line
header probe — because option A is 3 500 to 4 500 lines and five to seven
engine-weeks, which is roughly 200 lines of hand-rolled, security-sensitive
decoder per file in the corpora this project pins, every one of them parsing
attacker-controlled input under ruling 1.

The owner chose A anyway. The argument that overrides ruling 3 is the same one
[10](10-mesh-shadings.md) made and is worth stating rather than implying: none
of the three pinned corpora samples the domains where JPEG 2000 actually
concentrates — geospatial imagery, medical DICOM export, and digital
preservation masters. 0.4 per cent is a fact about a browser's regression
suite, a conformance suite and a writer's test suite. It is not a fact about
the world, and unlike JBIG2 and meshes the 19 hits are *split* across two
corpora (`pdfjs` 12, `verapdf` 7), including the conformance one.

Nobody reading this should take the work below as evidence that the corpus
asked for it. It did not. It is recorded here so that a later reader knows
this was a choice.

## What is wrong

Nothing. `Capability::Jpx` is returned by `ImageCodec::Jpx.capability()`,
`apply_chain` hands the still-encoded bytes back as
`ChainOutput::EncodedImage`, and both the XObject path and the inline path in
`crates/tinker-pdf/src/resources.rs` draw the neutral placeholder with a named
codec. The degradation works exactly as ruling 2 designs it.

So this is a missing feature, and the only thing wrong with today's behaviour
is that it is not the feature. That matters more here than in any other gap in
the set, because the failure mode of a partly-built JPEG 2000 decoder is not a
visible break — see [below](#where-a-half-implementation-is-worse-than-none).

## The three questions, settled here

Gap 18 names two and its risk table names the way they get lost: the
fixed-point decision otherwise gets *"made implicitly by whoever writes the
wavelet"*. The third is the one that has sunk decoders before, and gap 18
names it too without answering it.

They are answered in [Design](#design) below, in full. In one line each:

1. **Fraction width.** Coefficient planes are `i32` in **Q12** (twelve
   fractional bits); the six 9/7 constants are `i64` in **Q24**; every product
   is formed in `i64` and rounded back to Q12. Component precision above 16
   bits is refused.
2. **Perceptual budget.** Three gates, all in `tools/pdfcmp`'s existing
   numbers: **byte-identical** against `opj_decompress` for reversible 5/3;
   **no sample more than one level of 255 from an `f64` reference of the same
   lifting steps, and at most 1 per cent differing at all**, for 9/7; and
   `pdfcmp --threshold 12 --budget 0.0005` — Tinker's own tolerance — against
   `opj_decompress` for lossy 9/7.
3. **Test material.** Hand-built codestreams from a test-only encoder, the
   Annex D context tables transcribed and asserted entry by entry (which is
   the part a round-trip provably cannot check), `openjpeg` invoked as a
   subprocess oracle under ruling 9 and never vendored, and gap 23's 19 real
   files as the acceptance number.

## Scope

- **The JP2/JPX container** (T.800 Annex I): the box structure, `jP `, `ftyp`,
  `jp2h` with `ihdr`, `bpcc`, `colr`, `pclr`, `cmap`, `cdef`, `res`, and the
  `jp2c` contiguous codestream. A bare J2K codestream with no boxes at all,
  which PDF also permits.
- **The codestream headers** (Annex A): SIZ, COD, COC, QCD, QCC, RGN detection,
  POC detection, TLM/PLM/PLT/PPM/PPT, COM, SOT, SOD, EOC — parsed or refused by
  name, never skipped silently.
- **Tier-2** (Annex B): packet headers, tag trees, precinct and code-block
  partitions, code-block inclusion, zero bit-planes, coding-pass counts,
  segment lengths, multiple layers, multiple tile-parts, SOP and EPH, and all
  five progression orders.
- **Tier-1** (Annex D): the MQ coder from [17](17-jbig2-generic-region.md) with
  JPEG 2000's own context numbering and initial states, plus significance
  propagation, magnitude refinement and cleanup with run-length coding.
- **Dequantisation** (Annex E), reversible and irreversible, with the guard-bit
  arithmetic clamped before it reaches a plane.
- **The inverse wavelet** (Annex F): 5/3 reversible, exactly; 9/7 irreversible
  in fixed point; symmetric extension at every boundary.
- **The colour pipeline** (Annex G and I): DC level shift, inverse RCT and
  inverse ICT, component subsampling, `pclr`/`cmap` palettes and `cdef`
  channel definitions.
- **The PDF boundary** (ISO 32000-1 8.9.5.4 and 7.4.9): `/ColorSpace`
  interaction, `/SMaskInData`, and the rules about which image-dictionary keys
  a JPX stream overrides.
- **A refusal, everywhere else** — the feature, not the absence of one.

## Non-goals

JPEG 2000 is enormous, and a plan that does not say what it refuses will be
read as promising all of it. Each of these returns
`Unsupported(Capability::Jpx)` and leaves the placeholder standing.

- **Any encoder.** Nothing in the shipped surface encodes JPEG 2000. A
  test-only encoder exists for round-trips, exactly as `mq::encoder` does for
  JBIG2, and it is not part of the public crate.
- **ISO/IEC 15444-2 (Part 2) extensions.** Arbitrary transformation kernels
  (ATK/ATD), arbitrary decomposition (ADS), non-linearity points, and
  multiple-component transforms beyond RCT and ICT. A `jpx ` brand in `ftyp`
  is not itself a refusal; a Part 2 *marker* is.
- **The other parts of the family**: JPM (15444-6), Motion JPEG 2000
  (15444-3), JPIP (15444-9), and JPX's animation, compositing and layering
  boxes. PDF carries a still image.
- **Region of interest.** The RGN marker shifts coefficients by a scaling
  value; a decoder that ignores it produces a picture with a bright rectangle
  in it. Refused.
- **Resolution-limited or layer-limited decoding as a feature.** The decoder
  decodes what it is given, at full resolution. Gap 18 already costs the
  lowest-resolution shortcut and rejects it: it saves 15 per cent of the work
  for an image that looks broken.
- **Component precision above 16 bits.** T.800 allows up to 38; PDF's sample
  path reads at most 16 (`/BitsPerComponent`), and the fixed-point format
  below is proved for 16. Above that is refused, not truncated.
- **ICC colour management.** A `colr` box with method 2 is read for its
  component count and reported; conversion is `tinker-pdf-color`'s business
  and 15444-1 does not change that. Ruling 2 says report, not guess.
- **Streaming or incremental decode, and threads.** One shot, in one thread,
  like every other decoder in this crate.

## Design

### Where the code goes

`crates/tinker-pdf-filters/src/jpx/`, as a directory from the first commit
because it is six or seven files: `boxes.rs` (Annex I), `codestream.rs`
(Annex A), `tier2.rs` (Annex B), `tier1.rs` (Annex D), `quant.rs` (Annex E),
`dwt.rs` (Annex F), `colour.rs` (Annex G). One entry point,
`jpx_decode(input, limits) -> Result<JpxImage, FilterError>`, re-exported from
the crate root beside `jpeg_decode`, `ccitt_decode` and `jbig2_decode`.

Ruling 8 binds it: no COS type crosses the boundary and no PDF vocabulary
appears in the signature. `JpxImage` carries width, height, colour component
count, an output precision of 8 or 16, interleaved samples, an optional
opacity channel with the `cdef` type that named it, and a `JpxColour`
enumeration this crate owns. **`/SMaskInData` never appears in this crate.**
Deciding what the opacity channel is *for* is 8.9.5.4's rule and it lives in
`resources.rs`, in the same function that reads `/ColorSpace` — the same split
gap 17 made for JBIG2's polarity, and for the same reason.

### The MQ decoder is already built — and needs one thing it has not got

[17](17-jbig2-generic-region.md) put it in
`crates/tinker-pdf-filters/src/mq.rs` as its own module from its first commit
*precisely so this plan would not open with a refactor*. T.88 Annex E and
T.800 Annex C are the same coder, the same 47-row Qe table, the same INITDEC.
Its surface is `MqContext`, `MqContexts`, and
`MqDecoder::{new, position, decode, decode_at}`, and context *numbering* is
left entirely to the caller, which is the property that lets two codecs share
it.

**JPX's numbering is T.800 Annex D, and it is nineteen contexts:**

| Range | What | Table |
| --- | --- | --- |
| 0–8 | Zero coding, nine contexts chosen from the counts of significant horizontal, vertical and diagonal neighbours — with **three different mappings**, one for LL and LH bands, one for HL, one for HH | D.1 |
| 9–13 | Sign coding, five contexts, each with an XOR bit that flips the decoded sign | D.3 |
| 14–16 | Magnitude refinement, three contexts | D.4 |
| 17 | Run-length, used by the cleanup pass when a whole column of four is insignificant with an insignificant neighbourhood | D.2 |
| 18 | UNIFORM, used for the two bits that locate the first significant coefficient after a run, and for segmentation symbols | D.2 |

**One API change is needed, and it is the reason to say this here rather than
find it in week three.** T.88 E.3.6 initialises every context to state 0 with
MPS 0, which is what `MqContexts::new(len)` gives and what makes JBIG2's
numbering a free choice. JPEG 2000 does **not** start there: three contexts
have fixed non-zero initial states — the all-insignificant zero-coding context
at state 4, the run-length context at state 3, and UNIFORM at state 46 — and
everything else at state 0. There is no way to express that through the module
today. It gains one method, of the shape

```rust
impl MqContexts {
    /// One context's starting state (T.800 Table D.7). `state` is a row of
    /// Table E.1 and is refused above 46.
    pub fn set_state(&mut self, index: usize, state: u8, mps: u8);
}
```

and `reset()` must then return to the states the caller set rather than to
zero, or a code-block after the first decodes against the wrong initial
probabilities. **T.88 Annex H.1 and H.2 stay green through that change.** They
are permanent tests, they are what gap 17 built them to be, and a change made
for JPX that silently altered JBIG2 is the exact failure a shared module
invites. They are re-run as an exit criterion of the milestone that touches
`mq.rs`, not merely left in the suite.

### Question 1 — the fixed-point fraction width for the 9/7

Ruling 4 requires bit-identical output on linux, windows, macos and wasm. The
9/7 irreversible filter is specified with floating-point lifting coefficients
(T.800 Table F.4), so it cannot be built that way here. [13](13-quadratic-path-verb.md)
proved the cost of getting this wrong one dimension down, and its lesson is
the general one: a float comparison that lands differently on a 32-bit target
does not shift a result slightly, it changes a *count* — there, the number of
flattened segments; here, it would change which quantisation bucket a
coefficient lands in and therefore which integer a sample rounds to.
[12](12-image-sampling.md) made the same move for its pyramid level count, on
integers, for the same reason.

**The decision: coefficient planes are `i32` in Q12. The six 9/7 constants are
`i64` in Q24. Every product is formed in `i64` at Q36 and rounded back to Q12.
The reversible 5/3 path shares the plane type at Q0, since it is exact
integer arithmetic and wants no fraction at all.**

Here is the arithmetic that fixes those two numbers.

**Integer bits, from the dynamic range.** After the DC level shift a sample of
precision *R* has magnitude at most 2^(R−1). T.800 E.1 gives each subband a
nominal dynamic-range gain — 1 for LL, 2 for HL and LH, 4 for HH — so a
coefficient is at most 2^(R+1), and the guard bits the QCD marker signals
(conventionally two) let a conformant coder carry up to two bits more. This
build **clamps a dequantised coefficient to ±2^(R+2)** — nominal range plus one
guard bit — before it reaches a plane, and reports the clamp. The clamp is not
tidiness: it is what turns everything below from an assumption into a bound,
because the exponent and mantissa in a QCD marker are attacker-controlled and
a hostile stream can otherwise ask for any magnitude at all (ruling 1).

At the maximum supported *R* = 16 the clamp is 2^18, so a plane entry needs 18
integer bits and a sign. `i32` has 31 bits below the sign, leaving **13** for
the fraction; take **12** and keep a bit spare. At *R* = 8 the same format uses
23 of the 32 bits, and it is deliberately the *same* format at every
precision — a width that varies with the file is a second code path and a
second thing to get wrong.

**Fractional bits, from the precision the output needs.** Two error sources.

*Rounding the data.* Each store back to the plane rounds to 2^−13 sample
units. The longest path from a coefficient to an output sample is five
decomposition levels × two dimensions × six multiplies, sixty rounding sites,
and an error introduced at a coarse level is amplified by the synthesis
filters of every level below it — bounded by about 2^3 across five levels for
the 9/7's tap magnitudes. Sixty sites at 2^−13 with amplification 2^3 is
2^−13 × 2^9 = **2^−4 ≈ 0.06 sample units**, worst case, against half a level.

*Rounding the constants.* This is the term that scales with the coefficient
magnitude, and it is the one that decides Q24. A constant quantised to Q
fractional bits is wrong by at most 2^−(Q+1), and a lifting step computes
`X + c·(A + B)`, so the error is 2^−(Q+1)·2·max|coefficient| ≈
2^(R+2−Q). At Q24 and *R* = 8 that is 2^−14 per multiply, which over the same
sixty sites and the same 2^3 amplification is 2^−5 ≈ **0.03 sample units** —
comfortably under an eighth of an output level. At *R* = 16 it is 2^−6 per
multiply and 8 sample units in total, which is 1.2 × 10^−4 of a 16-bit full
scale, or three hundredths of a level once it reaches an 8-bit surface.

At Q16 the same figure would be 1.9 sample units for an 8-bit image — visibly
past a level, and past the budget in the next section. **That is the whole
reason the constants are wider than the data**, and it is the counter-intuitive
half of the answer: the data need only enough fraction to stay under an output
LSB in absolute terms, while the constants need enough to stay under it in
*relative* terms against the largest coefficient in the file.

The six constants, as `round(c · 2^24)` from T.800 Table F.4's decimals, with
the residual error each carries:

| Constant | Value | Q24 | Error |
| --- | --- | --- | --- |
| α | 1.586134342059924 | 26 610 918 | 2.75e−8 |
| β | 0.052980118572961 | 888 859 | 6.38e−9 |
| γ | 0.882911075530934 | 14 812 790 | 1.06e−8 |
| δ | 0.443506852043971 | 7 440 810 | 1.52e−8 |
| K | 1.230174104914001 | 20 638 897 | 1.93e−8 |
| 2/K | 1.625786132231922 | 27 276 165 | 6.57e−9 |

Every one is below 2^−25, which is the bound the paragraph above assumes. The
table is asserted in a test that recomputes `round(c · 2^24)` from the decimal
literals rather than restating the integers, so a transcription slip in the
constants cannot agree with a transcription slip in the test.

The two scaling constants are *K* on the low samples and *2/K* on the high
ones, in the inverse's step order (F.3.8.2: scale, then δ, γ, β, α). This is
not 1/*K* and *K*: JPEG 2000 normalises the analysis lowpass to DC gain 1 and
the analysis highpass to Nyquist gain 2, and the factor of two rides on the
high band's scaling. Getting this backwards produces an image with correct
structure at half or double contrast, which reads as a colour-management
problem rather than a wavelet problem — so it is pinned against the float
reference at milestone 5 rather than left to inspection.

**Where it must widen, and the overflow proof.** The multiply is the only
place, and it is a proof rather than a hope because the input is clamped.

- A plane entry is at most 2^18 sample units, which is **2^30** in Q12.
- The two scaling steps come first and multiply by at most 2/*K* = 1.6258,
  giving 2^30.71.
- The four lifting steps each compute `X ← X + c·(A + B)`, so each multiplies
  the running bound by (1 + 2|c|): ×1.887 for δ, ×2.766 for γ, ×1.106 for β,
  ×4.172 for α — **24.08 in total, 4.59 bits** — giving 2^35.30.
- That value times α at Q24 (2^24.67) is **2^59.96**, against `i64`'s 2^63.
  **Three bits — a factor of eight — of headroom, for the worst case a clamped
  input can construct.**

So: the line the lifting runs over is an `i64` scratch buffer at Q12, one row
or one column long, and it is `i64` rather than `i32` because the running
value grows by those 4.59 bits inside a pass and does not fit the plane's own
format mid-flight. The plane itself stays `i32`, which is what keeps a
4096 × 4096 four-component tile at 268 MB of scratch rather than 536 MB — and
the total is bounded anyway, see [Bounds](#bounds-per-ruling-1).

Rounding back is `(product + (1 << 23)) >> 24` — round-half-up on an arithmetic
shift, which Rust guarantees for signed integers and which is therefore the
same on every target. The final sample is
`clamp((coeff + (1 << 11)) >> 12) + (1 << (R − 1))` into `0 ..= 2^R − 1`. Both
roundings are stated here because "round to nearest" has three meanings and
only one of them is a specification.

**What carries it, in one place:**

```rust
/// Fractional bits in a 9/7 coefficient plane. 5/3 planes are Q0.
const Q: u32 = 12;
/// Fractional bits in the six constants of T.800 Table F.4.
const QC: u32 = 24;
/// Plane storage. Clamped to ±2^(R+2); at R = 16 that is 2^30 in Q12.
type Coeff = i32;
/// The lifting line, and every product. Bounded above by 2^59.96.
type Wide = i64;
```

### Question 2 — the perceptual budget, against a float reference

Gap 18 pre-argues the obvious oracle away: a fixed-point 9/7 differs from
every float-based reference decoder, OpenJPEG included. So the tolerance is
defined here, before implementation, and it is defined in the numbers the
repository already has rather than in a second standard. `tools/pdfcmp`
reports the fraction of pixels where any channel moved by more than
`--threshold` (default 12 of 255) and gates it against `--budget` (default
0.0005, which is Tinker's own `visual_regression.rs` tolerance), alongside the
mean, the worst pixel and its position, and the fraction differing at all.

**Three gates, in increasing looseness, and the first is the strongest thing
in this plan.**

**1. Reversible 5/3, against `opj_decompress`: byte-identical.** A losslessly
coded 5/3 stream has exactly one correct answer, and both decoders compute it
in integers. There is no tolerance to negotiate. `pdfcmp --threshold 0
--budget 0` is the spelling, and it must pass on every reversible fixture.
This single gate independently pins the container walk, every marker, tier-2's
packet headers and tag trees, tier-1's context numbering, dequantisation, the
inverse RCT and the DC level shift — **everything except the 9/7 arithmetic** —
against a decoder that shares no code with this one. It is the reason the
milestone order below puts 5/3 before 9/7.

**2. Irreversible 9/7, against an `f64` reference of the same lifting steps,
written in the test module.** The reference performs F.3.8.2's six steps in
`f64` on the same dequantised coefficients, so the *only* difference is the
arithmetic. The budget:

- **No output sample differs by more than one level of 255.** In pdfcmp:
  `--threshold 1 --budget 0` passes — no pixel moves by two levels or more.
- **At most 1 per cent of samples differ at all**, i.e. `differing ≤ 0.01`.

Both are far inside what the previous section bounds — the worst case there is
0.09 sample units for an 8-bit image, an eleventh of a level — so the gate is
set an order of magnitude above the proof, which means a failure is evidence
of a defect and not of the tolerance being fiddled. The measured numbers go
into the `As built` section, as gap 12 and gap 13 recorded theirs.

The `f64` reference is test-only and must stay so: shipping it would put a
float on a pixel path and `cargo xtask libm` exists to stop that.

**3. Irreversible 9/7, against `opj_decompress`: pdfcmp's defaults.**
`--threshold 12 --budget 0.0005`. Looser than gate 2 deliberately, because
this comparison contains differences that are not arithmetic: T.800 E.1.1.2
leaves the reconstruction point inside a quantisation interval to the decoder,
and at low bit rates with truncated bit-planes that choice moves pixels by
more than the wavelet ever will. This build uses *r* = 0.5, which is the
conventional choice and OpenJPEG's; the gate is set where it is so that a
disagreement about *r* on a heavily truncated stream does not read as a
wavelet bug.

**And ruling 4 on top of all three.** A JPX fixture joins
`crates/tinker-pdf/tests/determinism.rs` as the repository's **twelfth**
fingerprint, and `wasm32-wasip1` under wasmtime must reproduce it byte for
byte against native Windows — 32-bit against 64-bit, which is the `usize`
case. None of the other eleven may move. If the two targets disagree, the
table is not updated: it is the only thing in the repository that would ever
report a determinism bug.

### Question 3 — what the decoder is checked against

**The repository holds zero JPX bytes.** ISO/IEC 15444-4 conformance
codestreams are not freely redistributable, so they are not the answer and no
amount of wanting them makes them one. [17](17-jbig2-generic-region.md) faced
this exactly and solved it by transcribing T.88's Annex H by hand — 860 bytes,
twenty-one segments — and its `As built` records both what that cost and the
thing it bought that nothing else could: Annex H.1's first two pages are *the
same image coded two different ways*, so two decoders sharing no code had to
agree pixel for pixel. T.800 has no equivalent published datastream, so this
plan cannot copy that move. It has four sources instead, and the second one is
the important one.

**a. Hand-built codestreams, with a test-only encoder.** A minimal writer for
SIZ/COD/QCD/SOT/SOD/EOC, a forward DWT, and a tier-1 encoder for the three
passes, all in `#[cfg(test)]` — the same shape and the same justification as
gap 17's `mq::encoder`, which is not part of the shipped surface and exists so
that a round-trip means something. It is what makes the small cases testable:
a one-code-block image, a single tile, one decomposition level, a known
coefficient set whose expected output is derived by arithmetic rather than by
comparison.

**And gap 17 already proved the limit of this, which is the reason (b)
exists.** A round-trip cannot see a bijective relabelling of the context
array: every adaptive state starts identical, so an encoder's slot histories
and a decoder's stay in step under *any* permutation of the numbering. Gap 17
transposed two of template 0's context bits and Annex H.1 still decoded to its
published picture byte for byte, with every round-trip still passing. The same
hole exists here, and it is wider, because JPEG 2000 has three different
zero-coding mappings and a sign-coding table with an XOR bit.

**b. Annex D's tables, transcribed and asserted one entry at a time.** Gap
17's decisive test was a transcription of T.88 Figures 8 to 11 that sets one
pixel at a time and demands the bit the figure assigns it; it caught three
injected defects nothing else in the repository caught. The JPX equivalent is
the same test written against Annex D: for each of the three subband
orientations, every row of Table D.1 asserted against a neighbourhood
constructed to match it; every row of Table D.3 with both its context and its
XOR bit; Table D.4's three refinement contexts including the
first-refinement distinction; and Table D.7's three non-zero initial states.
**This is the test that catches what the round-trip cannot, and it is not
optional.** It is milestone 3's exit criterion, stated as such.

**c. `openjpeg` as a subprocess oracle, under ruling 9 — invoked, never
vendored.** This is exactly the shape [20](20-linearization-validation.md)
used for qpdf and [23](23-corpus-runner.md) for its corpora: an external CLI
in CI, nothing linked, outputs transient. `opj_compress` generates
codestreams that this repository did not encode, which is what covers (a)'s
blind spot, and it does so across the axes hand-built streams will never
naturally reach — each of LRCP, RLCP, RPCL, PCRL and CPRL; multiple tiles;
multiple quality layers; explicit precinct sizes; SOP and EPH; code-block
sizes from 4 × 4 to 64 × 64; one to five decomposition levels; reversible and
irreversible. Every one of those is a flag, and every one of them is a
progression order or a partition this decoder would otherwise be guessing
about. `opj_decompress` then supplies the reference image for gates 1 and 3.

Two things about this, both learned here:

- **The codestreams `opj_compress` produces from our own images are ours to
  commit**, and they seed the fuzz corpus. That is not redistributing
  conformance material; it is a tool's output on our input. The ISO files stay
  out.
- **The CI job must go red when `openjpeg` is absent, not green.** Gap 20 found
  this the hard way with qpdf: a skipped test exits 0 and reads exactly like a
  pass, so `crates/tinker-pdf-filters/tests/jpx_oracle.rs` prints
  `jpx-oracle: RAN`, the job greps for it, and it fails on `jpx-oracle:
  SKIPPED`. Measured with `openjpeg` off `PATH` before the job is believed,
  the way PRE-B measured the wasm job's ability to tell a run from a non-run.

**d. Gap 23's 19 real files, as the acceptance number.** Not committed — they
are in the pinned corpora, fetched. They are the only real-world JPX within
reach, and the question they answer is the one gap 17's `As built` answered
for JBIG2 and had to answer explicitly because "JBIG2 works now" would
otherwise have been inferred: **how many of the 19 decode, and what each of
the rest is refused for, by name.** That number goes in the `As built`. It is
a claim about 19 files, and the `As built` must say so rather than let it read
as a claim about JPEG 2000.

### Bounds, per ruling 1

Image and tile dimensions are attacker-controlled 32-bit values, and the fuzz
campaign landed the general lesson hours before this plan was written:
`5adf502 fix(render): bound the group buffers a page may open, not just their
depth` — an 1 851-byte page that took 19.3 seconds to render 9 600 pixels,
inside a depth cap that never fired, because **a depth or per-item cap is not
a work cap once the structure branches**. `MAX_TILE_WORK`,
`MAX_GROUP_BUFFERS` and `MAX_MESH_TRIANGLES` all exist for that reason and all
three say so in their own comments.

JPEG 2000 is the worst branching structure in the engine. A codestream
multiplies **tiles × components × resolutions × precincts × code-blocks ×
layers × bit-planes × three coding passes**, and every one of those factors is
individually bounded by the standard while their product is not: 65 535 tiles
is legal, 16 384 components is legal, 33 resolutions is legal, and a
code-block may carry 31 bit-planes.

So there is a **total**, spent and never refunded:

| Name | Bounds | Why it cannot be a per-item cap |
| --- | --- | --- |
| `MAX_JPX_SAMPLES` | Tile-component samples summed over every tile and component, checked with `checked_mul` before any plane is allocated | One tile inside the ceiling times 16 384 components is not |
| `MAX_JPX_CODE_BLOCKS` | Code-blocks over all tiles, components, resolutions, precincts and layers | A precinct's code-block count is small; the precinct count is not |
| `MAX_JPX_WORK` | **The real budget**: coefficient × coding-pass, charged as tier-1 runs and checked before each pass | Bit-planes per code-block is capped at 31 and code-blocks are capped, and 31 × the cap is still unbounded work |

Per-item caps exist beside them — tiles, components, decomposition levels,
code-block dimensions — and the comment on each says in as many words that it
is *not* the work cap, in the register `MAX_MESH_TRIANGLES` already uses.

Three more edges, all of which are arithmetic on attacker numbers rather than
policy:

- **The tile grid.** A.5.1 constrains `XTsiz > 0`, `XTOsiz ≤ XOsiz < Xsiz`,
  and the same on Y. A zero `XTsiz` is a division by zero and `XOsiz > Xsiz`
  is a subtraction underflow. Every one of A.5.1's constraints is checked, not
  assumed, and a violation refuses the file.
- **Every plane allocation goes through a checked multiply against the output
  ceiling before it happens** — the pattern `ccitt.rs` established and
  `jbig2.rs` reused as `packed_size`.
- **Packet and segment lengths.** A code-block's declared length is checked
  against the bytes remaining before it is sliced, and a packet whose declared
  length does not land on the next packet's start refuses the tile rather than
  resynchronising. That is a bounds check *and* an integrity check; see the
  next section for why the second half matters more.

A `jpx` fuzz target — the repository's **eighteenth** — lands with the first
milestone, not at the end, per plan 02's rule that every decoder gets one the
day it exists.

### The PDF boundary

ISO 32000-1 8.9.5.4 makes JPX the one codec that overrides its container, and
each of its rules is a test:

- **`/ColorSpace` present wins.** The codestream's own `colr` is ignored, and
  the component count must agree or the image is refused.
- **`/ColorSpace` absent takes the space from `colr`** — enumerated sRGB,
  greyscale, sYCC, e-YCC or CMYK. A method this build cannot map is
  *reported*, not guessed at (ruling 2). This is the one thing gap 18's option
  B was going to buy on its own, and it arrives here.
- **`/BitsPerComponent` is ignored**; the codestream's precision decides.
- **`/Decode` is ignored** unless the image is an `/ImageMask`.
- **`/SMaskInData`**: 0 ignores any opacity channel the codestream carries; 1
  uses it as a soft mask; 2 uses it and un-premultiplies the colour channels
  before compositing. `/SMask` on the same dictionary takes precedence over
  all three, with a warning recording that both were present.

Output joins the generic sample path in `resources.rs` behind a single
`jpx_samples(...)`, the way `ccitt_samples` and `jbig2_samples` already do —
one entry point, so the inline path and the XObject path cannot drift, which
is the failure gap 08 found and plan 02's amendment records as the one to
watch for here.

Components with `XRsiz`/`YRsiz` above 1 are upsampled to the reference grid by
**replication**, not by a triangle filter. That is the opposite of plan 02's
JPEG decision and for the same underlying reason: there the oracle is
libjpeg-turbo, which does fancy upsampling, and here the oracle is
`opj_decompress`, which replicates. Gate 1's byte-identical claim is only
reachable if the two agree.

## Where a half-implementation is worse than none

Every plan in this set has this section and they have repeatedly been the most
valuable part. For JPEG 2000 the hazard is sharper than for any other codec
here, and it is worth being precise about why.

**A wrong JPEG 2000 decode looks like a photograph.**

That is the whole of it. A wrong Huffman table produces obvious garbage; a
wrong CCITT mode code produces streaks; a JBIG2 file with no decodable region
produces a blank page, which gap 17 correctly identified as the thing to
refuse. But tier-2 hands tier-1 a byte range, tier-1 hands the wavelet a set
of coefficients, and **the inverse wavelet is a smoothing operator**: it turns
wrong coefficients into a soft, low-frequency, plausible image. A decoder that
mis-parses a packet header for a progression order it did not implement, or
that assumes a default precinct partition where the COD signalled one, does
not fail. It produces a blurry picture that a user cannot distinguish from a
bad scan, in a document where the real image was a chest radiograph or a map.

The placeholder is honest. A plausible wrong picture is not, and it is not
recoverable by the reader either, because nothing downstream knows.

**So the refusal path is the feature, exactly as it was for gap 17, and it is
enumerated rather than left as a default.** `Unsupported(Capability::Jpx)` is
returned — and the placeholder drawn — for every one of:

- a progression order not implemented, and any POC marker, which changes the
  order mid-stream;
- any code-block style bit in COD/COC Table A.19 this build does not
  implement: selective arithmetic-coding bypass, context reset on each pass,
  termination on each pass, vertically causal context, predictable
  termination, segmentation symbols;
- an RGN marker, a Part 2 marker, or a marker the standard defines and this
  build does not;
- tile-parts out of order, or a tile whose parts do not cover it;
- a packet whose declared length does not land where the next packet begins;
- a `colr` method that cannot be mapped, when `/ColorSpace` is absent;
- component precision above 16 bits, or a component count the colour pipeline
  cannot interpret;
- any budget in the previous section being spent.

**Two of those are integrity checks rather than capability checks, and they
are the cheapest real defence this plan has.** The packet-length check catches
a mis-parsed header *before any pixel exists*, because a tier-2 parser that
has gone wrong almost never lands on the next packet boundary by accident.
And segmentation symbols, when a stream signals them (Table A.19, bit 0x20),
end every cleanup pass with the four decisions `1010` in the UNIFORM context —
decoding them and **checking them** is a free per-code-block verification that
the arithmetic decoder is still in step. A build that decodes the symbols and
discards them has thrown away the one thing in the format that tells it the
picture is wrong.

**The second hazard is the reporting one, and gap 18 already names it for
option B.** If this decodes the common case and refuses a large share of real
JPX, STATUS and the capability surface must say which — measured against gap
23's 19 files, written down in the `As built`, not inferred from "JPX works
now". Gap 17's `As built` does exactly this for the generic-region lineage and
says plainly that it decodes the minority of real JBIG2. The same paragraph is
owed here.

**Third, and stated so it is not rediscovered:** partial decode to a lower
resolution level is not a fallback. Gap 18 costs it — it saves the inverse
wavelet, about 15 per cent of the work, and needs everything else — and the
result is an image at the wrong size or a soft one at the right size, both of
which are the plausible-wrong-picture failure above wearing a different hat.

## Milestones

The commit-boundary rule is per-plan
([00-execution-order.md](00-execution-order.md)), and this plan is emphatically
one commit per milestone: eight commits, each independently green under the
full gate.

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Container and codestream headers; the `jpx` fuzz target | A hand-built JP2 reports dimensions, component count and precision exactly, and a bare J2K codestream with no boxes is recognised too; SIZ/COD/COC/QCD/QCC/SOT/SOD/EOC parse with COC and QCC overriding per component; every marker in Table A.2 is either parsed or **named** in a refusal, never skipped silently; A.5.1's tile-grid constraints each refuse a violating file rather than dividing by zero; `cargo fuzz run jpx` builds and runs on the committed seeds | M |
| 2 | Tier-2: tag trees, packet headers, precincts, code-block partition, five progression orders | A tag tree reproduces a worked B.10.2 example decoded by hand; packet headers yield inclusion, zero-bit-planes, pass counts and segment lengths for streams `opj_compress` emitted in each of LRCP, RLCP, RPCL, PCRL and CPRL, with multiple tiles, multiple layers and explicit precincts; SOP and EPH consumed when signalled and refused when signalled-but-absent; **a packet whose declared length does not land on the next packet's start refuses the tile** | L |
| 3 | Tier-1: Annex D contexts and the three coding passes; the `mq.rs` initial-state API | Every row of Tables D.1 (all three orientations), D.3 with its XOR bit, D.4 and D.7 asserted **one entry at a time against the standard**, not through a round-trip — the test gap 17 proved a round-trip cannot replace; significance propagation, magnitude refinement and cleanup with run-length reproduce a code-block the test encoder built; **T.88 Annex H.1 and H.2 re-run green** after `MqContexts` gains `set_state` | L |
| 4 | Dequantisation and the inverse 5/3 | A losslessly coded 5/3 stream from `opj_compress` decodes **byte-identical** to `opj_decompress`'s output, at one, three and five decomposition levels, with and without the RCT — the gate that pins milestones 1 to 3 against a decoder sharing no code; a QCD exponent or guard-bit count that would overflow is clamped before it reaches a plane, and the clamp is reported | M |
| 5 | The inverse 9/7, in Q12 | Against an `f64` reference of F.3.8.2's own steps: **no sample differs by more than one level of 255, and at most 1 per cent differ at all**; against `opj_decompress`, `pdfcmp --threshold 12 --budget 0.0005` on every lossy fixture; the six constants equal `round(c · 2^24)` recomputed in the test from Table F.4's decimals; a `debug_assert` pins the `i64` product bound at the clamped worst case | M |
| 6 | The colour pipeline | Inverse RCT and inverse ICT; DC level shift for signed and unsigned `Ssiz`; subsampled components replicated to the reference grid; `pclr` and `cmap` palettes; `cdef` naming the opacity channel and its premultiplication type; a `colr` method or enumerated space this build cannot map is **reported**, and the image refused rather than rendered in a guessed space | M |
| 7 | The PDF boundary | 8.9.5.4, one test per rule: `/ColorSpace` present overriding `colr` and refused on a component-count mismatch; `/ColorSpace` absent taking sRGB, greyscale and CMYK from `colr`; `/BitsPerComponent` and `/Decode` ignored; `/SMaskInData` 0, 1 and 2 each asserted on pixels, with 2 un-premultiplied; `/SMask` beating `/SMaskInData` with a warning; one `jpx_samples` entry point reached by both the XObject and the inline path | M |
| 8 | Bounds, refusal, determinism, and the number | `MAX_JPX_WORK` refuses a branching stream that stays inside every per-item cap, asserted by the *warning* rather than by a clock (`5adf502`'s method); every entry in the refusal list returns `Unsupported(Capability::Jpx)` and draws the placeholder, each with a test; the **twelfth** determinism fingerprint, reproduced byte-for-byte on `wasm32-wasip1` with none of the other eleven moving; `cargo fuzz run jpx` survives a session with no crash, no OOM and no timeout; **how many of gap 23's 19 files decode, and what each of the rest is refused for, written down** | M |

Milestone 4 is the hinge. Until it passes, nothing in 1 to 3 has been checked
against anything outside this repository; once it passes, all of it has been.
That is why the reversible wavelet comes before the irreversible one even
though the irreversible one is the interesting problem.

## Dependencies

**Needs first — all landed:**

- [17](17-jbig2-generic-region.md) for `crates/tinker-pdf-filters/src/mq.rs`,
  which is a shared module for this plan's benefit and needs one method added.
- [16](16-ccitt-completion.md) for the packed-sample conventions and the
  `ccitt_samples` shape that `jpx_samples` follows.
- [23](23-corpus-runner.md) for the 19 files this is measured against, and for
  the hit rate the first section records.
- [24](24-fuzz-execution.md) M1–M4 for the fuzz toolchain. `cargo-fuzz` needs
  libFuzzer, which is not supported on `x86_64-pc-windows-msvc`; WSL2 with
  nightly is the local route, as three other plans already record.
- [25](25-wasm-determinism-leg.md) M1–M3 for the wasm determinism leg the
  twelfth fingerprint is checked on.

**Needs, and is not in the repository:** `openjpeg` (`opj_compress` and
`opj_decompress`) on `PATH` or at `TINKER_OPENJPEG`, as a CI subprocess under
ruling 9. Nothing links it and nothing vendors it.

**Amends, in the same commits:** `docs/plans/02-filters.md`'s non-goals, whose
JPXDecode entry currently says this crate returns `Unsupported` for a JPX
probe — which stops being true, in exactly the way and for exactly the reason
the JBIG2 half of that entry was amended in August. The capability still
answers `Some(Capability::Jpx)`; what changes is that it now means "this crate
may refuse these bytes", not "this crate will never decode them".

**Unblocks:** nothing.

## Risks

| Risk | Mitigation |
| --- | --- |
| The fixed-point width is decided implicitly by whoever writes `dwt.rs` — gap 18's own risk row, and the reason this plan exists | Decided above with the arithmetic behind it: `i32` Q12 planes, `i64` Q24 constants, `i64` products bounded at 2^59.96, and a clamp that makes the bound a proof rather than an assumption |
| A wrong tier-2 parse produces a plausible image rather than a visible failure, and nobody notices for months | The packet-length landing check and the segmentation-symbol verification, both integrity checks rather than capability checks; the enumerated refusal list; and milestone 4's byte-identical gate, which no mis-parse can pass |
| The round-trip against the in-tree encoder passes for the wrong reason — gap 17 proved a context relabelling is invisible to one | Milestone 3's exit criterion is Annex D's tables asserted entry by entry, and milestones 4 and 5 are against a decoder sharing no code |
| `openjpeg` is absent and the oracle job goes green | Gap 20's pattern, which was learned from qpdf doing exactly this: the test prints `jpx-oracle: RAN`, the job greps for it and fails on `SKIPPED`, and the job is measured with `openjpeg` off `PATH` before it is believed |
| A per-item cap is mistaken for a work cap and a branching codestream runs away | `MAX_JPX_WORK` is a total, charged as tier-1 runs, spent and never refunded — `5adf502`'s lesson, and `MAX_TILE_WORK`'s, and `MAX_MESH_TRIANGLES`' |
| `mq.rs` is changed for JPX and silently breaks JBIG2 — gap 17 named this when it built the module | T.88 Annex H.1 and H.2 are permanent tests and are an explicit exit criterion of milestone 3, re-run rather than assumed |
| The perceptual budget is negotiated downwards after the fact, once a fixture fails | Gates 1 and 2 are set an order of magnitude inside what the fixed-point bound proves, so a failure is evidence of a defect; gate 3's looseness has a stated cause (the reconstruction point *r*) rather than being slack |
| Five to seven engine-weeks and 3 500–4 500 lines for 0.4 per cent of the pinned corpora | Recorded at the top of this document rather than discovered later: built by decision, with the hit rate stated |
| Adding `Warning` or `Capability` variants is a public API change with exhaustive match sites | The compiler catches the filters-crate sites; check whether any consumer maps `Warning` with a wildcard arm, which would swallow the new ones — gap 17's finding |
| An `As built` that reads as "JPX works now" | Milestone 8's last exit criterion is a number about 19 files, and the `As built` must say it is a claim about 19 files |

## Progress — 16 August 2026

**Milestones 0, 1 and 2 have landed.** `As built` is deliberately not written
yet; it belongs to whoever finishes milestone 8.

- **M0** (`bfa73a2`) — `mq.rs` gained `set_state`, because T.88 starts every
  context at state 0 and T.800 does not: zero-coding at 4, run-length at 3,
  UNIFORM at 46. `reset()` returns contexts to the caller's configured states
  rather than to zero, since the second code-block in every stream would
  otherwise decode against the wrong probabilities. T.88's Annex H.1 and H.2
  are still green and now have two guards of their own —
  `annex_h2_is_unmoved_by_a_configured_neighbour` and
  `contexts_nobody_configured_still_reset_to_t88_zero` — so a change made for
  JPX cannot quietly break JBIG2.
- **M1** (`46364e3`) — every Table A.2 marker parsed or named in a refusal,
  A.5.1's tile-grid constraints refusing rather than dividing by zero, and the
  eighteenth fuzz target.
- **M2** (this commit) — tag trees, the packet bit reader with B.10.1's
  stuffing rule, precinct and code-block geometry anchored to the reference
  grid, packet headers, and all five of B.12's progression orders.

**Tier-2 runs even though nothing consumes its answer yet**, and that is
deliberate. It is where the integrity checks live: a packet that does not end
where the next begins is a codestream disagreeing with itself, and refusing
there means a malformed file is named rather than reaching a stage that would
smooth it into a photograph. It also makes the refusal honest —
`NotBuilt("tier-1")` names the one stage that is missing, where
`NotBuilt("tier-2 and everything after it")` named five and would have gone on
naming five after tier-2 existed.

**A wiring lesson worth keeping.** Two header fixtures started failing the
moment tier-2 was reached, because they carried no packet bytes at all — the
default spec is a 4x4 image with one decomposition level, so it has two
resolutions and owes two packets however empty the picture is. They were
sufficient for every stage before tier-2 and nobody had noticed they were
short. A test that starts failing when a stage is wired in is that stage doing
its job.

### What milestone 3 needs

- `CodeBlock::width`/`height`, `CodingStyle::segmentation_symbols` and
  `Codestream::quant_for` are written and carry `#[allow(dead_code)]` naming
  the milestone that reads them. Remove the attributes as they are consumed.
- The Annex D tables (D.1, D.3, D.4, D.7) must be asserted **entry by entry**,
  not by round trip. Gap 17 proved a round trip cannot see a bijective
  relabelling: it transposed two of JBIG2's template 0 context bits and T.88's
  Annex H.1 still decoded to its published picture byte for byte. JPX's
  exposure is wider — three zero-coding mappings plus a sign table with an XOR
  bit.
- Segmentation symbols, when signalled, are a free per-code-block check that
  the MQ decoder is still in step. Decode the `1010` in UNIFORM **and check
  it**.

### Milestone 3 — tier-1 (16 August 2026)

The three coding passes over the MQ coder, with Annex D's context formation,
and the `NotBuilt` refusal moved from `tier-1` to `dequantisation`.

**The evidence is the tables, entry by entry — not the round trip.** Both
exist and they catch disjoint things, which is worth stating because the round
trip is the one that *looks* conclusive:

- Transposing two of D.3's sign-coding contexts fails
  `table_d3_sign_coding_entry_by_entry` and `sign_coding_is_antisymmetric`,
  and **does not fail the round trip**. Relabelling a context array is a
  bijection: every adaptive state starts identical, so an encoder's slot
  histories and a decoder's stay in step under any permutation. Gap 17 proved
  the same thing on JBIG2 by transposing two template 0 bits and watching
  T.88's Annex H.1 still decode byte for byte.
- Starting the zero-coding context at T.88's 0 rather than T.800's 4 fails
  `table_d7_initial_states_entry_by_entry` and
  `a_reset_returns_to_the_states_t800_asked_for`.
- What the round trip catches and no table can: the scan pattern, the stripe
  order, the pass sequence, the run-length shortcut, sign propagation across a
  stripe boundary, and the segmentation symbol.

**One test was wrong before the code was.** The first `reset` test disturbed
the contexts with `set_state`, which *redefines the baseline* — its own
documentation says "the state survives `MqContexts::reset`", which is the
whole reason it exists. So the test would have passed against a `reset` that
did nothing at all. It now disturbs by decoding, and asserts the contexts
actually moved before asking `reset` to put them back.

### What milestone 4 needs

- `Codestream::quant_for` is written and carries
  `#[allow(dead_code, reason = "dequantisation, milestone 4")]`. Remove it.
- M4 is where the plan's **gate 1** finally becomes available: reversible 5/3
  against `opj_decompress`, byte-identical. Until coefficients become samples
  there is nothing to compare, which is why M3's evidence is tables and a
  round trip rather than an oracle.
- The coefficient planes are `i32` in Q12 and the 5/3 path shares the type at
  Q0 — it is exact integer arithmetic and wants no fraction.
