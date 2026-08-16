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

*A was chosen.* Both of those are settled in
[18a](18a-jpx-decoder.md) — along with a third this document does not name,
which is what a repository holding zero JPX bytes tests against. The second
one is already half-discharged: [17](17-jbig2-generic-region.md) put the MQ
decoder in `crates/tinker-pdf-filters/src/mq.rs` from its first commit, with
Annex H.1 and H.2 as permanent tests, so option A opens with a build rather
than a refactor.

## Milestones

Only for option B, since A needs its own plan written after the decision.
**A was the decision, and that plan now exists:
[18a](18a-jpx-decoder.md).** The three milestones below are still option B's
and are left alone — read 18a instead, not this table.

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

## Amendment — August 2026: the corpus evidence, and what it says about option A

[23](23-corpus-runner.md) has run — the report this document's decision was
always supposed to wait for. Across 4 525 documents from pdf.js, veraPDF,
qpdf's qtest and the PDF Association:

**JPX: 19 files, 0.4 %.** JBIG2 is 2.3 % over the same corpus; mesh shadings
are 0.2 %.

Set that against this document's own costing of option A: **3 500 to 4 500
lines and five to seven engine-weeks**, for a wavelet, an MQ coder, tier-1 and
tier-2 coding, and a colour pipeline — the largest single item in the whole gap
set, larger than the entire font lane that was just closed.

That is roughly 200 lines of hand-rolled, security-sensitive decoder per file
in the corpora this project pins, and every one of those lines is parsing
attacker-controlled input under ruling 1.

The evidence therefore argues for **option B**: the JP2 box walk, colour-space
inference from the boxes rather than the codestream, and honest reporting of
what a file needs and this build cannot decode. Option B is S-sized, its
milestones are already written in the table above, and it leaves the file
saying precisely what it is instead of a generic refusal.

Three qualifications, because a decision made on one number deserves them:

- The 19 hits are split `pdfjs` 12 and `verapdf` 7 — unlike JBIG2 and meshes,
  JPX appears in more than one corpus, including the conformance one.
- None of these corpora samples the domains where JPEG 2000 actually
  concentrates: geospatial imagery, medical DICOM, and digital preservation
  masters. A corpus of any of those would move this number sharply.
- The test-material problem this document already names is unchanged and is
  independent of the hit rate: ISO/IEC 15444-4 conformance codestreams are not
  freely redistributable, and a fixed-point wavelet will differ from every
  float-based reference decoder, so option A would have to define its own
  oracle before writing `dwt.rs`.

If option A is chosen anyway, the decision record and milestone table it needs
still do not exist — see the preamble note about a plan being written *after*
the decision, not before.

## Decided — August 2026: option A, and where its plan lives

**The owner chose A**, against the amendment above and against the corpus
number it rests on. This document stops being a live question at that point
and becomes the record of the choice.

The plan is [18a](18a-jpx-decoder.md), written as PRE-D in
[00-execution-order.md](00-execution-order.md) because the paragraph directly
above says the plan cannot exist before the decision does. It carries the
eight-milestone table this document deliberately does not have, and it answers
the two questions the "If A is chosen" section demands before any code — plus
the test-material question this document raises in its third qualification and
leaves open.

Three things belong here rather than only there, because they are properties
of the *decision* and not of the build:

- **The hit rate is unchanged and is stated in 18a's opening section.** 19
  files, 0.4 per cent, split `pdfjs` 12 and `verapdf` 7. A reader who comes to
  18a cold is told in its first paragraph that this was a choice rather than a
  finding, in the register [10](10-mesh-shadings.md)'s `As built` used for the
  same situation.
- **The argument that overrode ruling 3** is this document's own second
  qualification: none of the pinned corpora samples geospatial, medical or
  preservation material, which is where JPEG 2000 concentrates. That is a real
  argument about the corpora rather than a courtesy, and it is the same one
  gap 10 used. It is not evidence that the corpus asked for the work.
- **Nothing above is retracted.** The size estimate stands at 3 500–4 500
  lines and five to seven engine-weeks; 18a sizes eight milestones against it
  and does not argue it down.
