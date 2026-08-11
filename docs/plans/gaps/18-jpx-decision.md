# JPX: a decision, not a plan

JPEG 2000 in PDF. The engine reports it and draws a placeholder. This document
exists so the choice is made deliberately rather than drifted into, because
the honest estimate is large enough that starting it by accident would consume
a quarter of the remaining engine budget. (S — the document; the options
below are not)

## What is wrong

Nothing. `Capability::Jpx` is returned, the chain stops, the placeholder is
drawn and named. The degradation works exactly as designed.

## The size, stated honestly

A JPX decoder that produces a correct image needs, in order, all of:

- The JP2 container: box structure, `ihdr`, `colr`, `cdef` (T.800 Annex I).
- The codestream: SIZ, COD, QCD, SOT, SOP/EPH markers (Annex A).
- Tier-2: packet headers, tag trees, precinct and code-block partitions,
  coding-pass counts, code-block lengths, five progression orders (Annex B).
- Tier-1: the MQ coder plus three coding passes per bit-plane — significance
  propagation, magnitude refinement, cleanup with run-length — over the
  context tables in Annex D.
- Dequantisation (Annex E).
- The inverse wavelet: 5/3 reversible and 9/7 irreversible, with symmetric
  extension (Annex F).
- DC level shift and the inverse RCT and ICT (Annex G).

**3,500–4,500 lines, five to seven focused weeks.** Tier-1 alone is the size
of the entire JPEG decoder. Anyone estimating less has not written tier-2.

**There is no cheap partial that produces a real image.** The obvious idea —
decode only the lowest resolution level — still needs the container, the
codestream parser, tier-2, tier-1 and dequantisation, and saves only the
inverse wavelet: about 15% of the work, for an image that looks broken.

**And a determinism problem.** The 9/7 irreversible wavelet is specified with
floating-point lifting coefficients. Ruling 4 forbids that on a pixel path, so
it must be fixed-point — which means output will differ from every
float-based reference decoder, OpenJPEG included, by small amounts. The
fractional bit count and the perceptual budget have to be decided *before* the
wavelet is written; afterwards it becomes an argument about which decoder is
right.

## The options

**A. Full decoder.** 3,500–4,500 lines, 5–7 weeks. Reuses the MQ coder from
[17](17-jbig2-generic-region.md) — T.800 Annex C and T.88 Annex E are the same
coder. Everything else is new.

**B. Header probe only.** Parse the JP2 boxes and the SIZ marker: dimensions,
component count, bit depth, colour space, and `/SMaskInData`. Roughly 150
lines. The image still renders as a placeholder — but the placeholder is
correctly sized and the colour space is inferred rather than guessed, and
`/SMaskInData` stops being silently ignored.

The return is modest and worth being precise about: the placeholder is
*already* correctly sized and positioned by the CTM, because the image
dictionary carries `/Width` and `/Height`. So B buys colour-space inference
and `/SMaskInData`, and nothing visual.

**C. Nothing.** The current behaviour. Correct, reported, honest.

## Recommendation

**C now, B if the corpus asks, A only on evidence.**

Ruling 3 exists for exactly this: schedule a deferred capability by hit-rate,
not by ambition. The hit-rate report does not exist yet
([23](23-corpus-runner.md)), so today there is no evidence at all — and JPX is
the item in the whole remaining set where building on judgement would cost
the most.

JPX in PDF is concentrated: scanned archives that chose it over JBIG2, some
geospatial imagery, some medical. If the corpus says under a per cent, A is
five weeks for a rounding error and the placeholder is the right answer
indefinitely.

## Where a half-implementation is worse than none

**Calling B "JPX support".** The claim to avoid. A header probe decodes no
pixels; describing it as support means a user sees a placeholder in a feature
matrix that says the codec works. If B is taken, it must be recorded as
"dimensions and colour space only", in STATUS and in the capability surface.

**A partial A.** A lowest-resolution decode produces a visibly broken image
where a placeholder is a clean absence. There is no honest middle.

## If A is chosen

The decision that must be made first, before any code: **the fixed-point
fraction width for the 9/7 wavelet, and the perceptual budget against a
float reference.** Written down before `dwt.rs` exists.

Second: the MQ decoder moves to a shared module with T.88's Annex H.2 as a
permanent test, since a change for JPX would otherwise silently alter JBIG2.

## Milestones

Only for option B, since A needs its own plan written after the decision.

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | JP2 box walk and SIZ parse | A hand-built JP2 header reports dimensions, components and bit depth exactly; a raw J2K codestream is recognised too | S |
| 2 | Colour space inference and `/SMaskInData` | An image with no `/ColorSpace` takes it from `colr`; `/SMaskInData 1` is honoured or reported rather than ignored | S |
| 3 | Honest reporting | STATUS and the capability surface say "dimensions and colour space only", not "JPX" | S |

## Dependencies

**Needs first:** [23](23-corpus-runner.md) — this is a decision that wants
evidence, and the evidence is one corpus run away.

**Unblocks:** nothing.

## Risks

| Risk | Mitigation |
| --- | --- |
| Option A is started incrementally without a decision and consumes weeks before anyone notices | This document; the size is stated up front so the choice is explicit |
| Option B is described as support | Milestone 3 is the reporting, not an afterthought |
| The 9/7 fixed-point choice is made implicitly by whoever writes the wavelet | Named above as the first decision of option A, before any code |
