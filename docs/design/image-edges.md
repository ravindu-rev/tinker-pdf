# Image edges, sampling and conflation

The roadmap's Tier 1 item said image edges are quantised to whole device
pixels, and that partial coverage at the edge was the fix. Partial coverage was
built, and it is right — but measurement said it was neither the whole cause
nor, on its own, an improvement. This document records what the corpus actually
said, because the diagnosis is the part worth keeping.

## What the measurement found

Three separate defects sat behind eight failing files, and only the first is
the one the roadmap named.

**1. The outer edge was quantised.** A destination pixel was painted in full or
not at all, by whether the image's transformed unit square contained its
centre. Every filled path beside it was anti-aliased; images alone opted out.

**2. Every *internal* sample boundary was quantised too, and this was the
dominant term.** Above 1:1 the sampler took one nearest tap, so each sample's
footprint was assigned to whole device pixels — the same defect as the outer
edge, one level down, and there are as many internal boundaries as there are
samples. Isolated on a 512-square source, comparing a render against the
box-filtered render at twice the scale:

| destination | regime | pixels differing |
| --- | --- | ---: |
| 341, 300, 600 | 2x magnifies, 1x minifies | 49.9 %, 52.0 %, 21.2 % |
| 256, 128 | both minify | **0.00 %** |

Minification was already scale-coherent. Magnification was not, and only ~2 %
of the differing pixels were anywhere near a border. `large-inline-image.pdf`
is the proof in the corpus: one inline image of 49 x 56 samples stretched
across a 612 x 792 page, no abutment anywhere, and it was failing `dpi` at
3.2 %.

**3. Anti-aliasing an image edge *creates* an artefact — conflation.** Two
images sharing an edge each cover the boundary pixel partly, and compositing
them one after the other lets the page through between them: a half-and-half
boundary keeps a quarter of the backdrop. Quantised edges had no seam, because
the pixel belonged wholly to whichever strip held its centre. So partial
coverage alone made the strip files *worse* — `pclm-in.pdf` went from 17.3 % to
18.6 %, and `rotate` and `crop`, which had held, broke. The regression scaled
exactly with the number of abutting images: one image, no regression; several,
`rotate` broke; 415 strips, everything broke.

## Scope

- Geometric coverage for the image's own rectangle, from the crate's own path
  rasterizer rather than a second implementation.
- A sampling rule that does not change filter *family* across 1:1, so two
  renders at different scales agree.
- Accumulation across a run of image draws, so abutting ones do not conflate.
- Analytic tests for all three, every expectation computed from the geometry.

## Non-goals

- Removing conflation between an image and a *path*, or between images
  separated by other drawing. A run ends at anything that paints, and it must:
  fragments are added, and z-order is not negotiable.
- Smoothing a magnified image. `/Interpolate false` means hard samples and
  still gets them; what changed is that a sample's *edge* is now anti-aliased
  rather than snapped.
- Making `dpi` hold on every image file. It does not — see the numbers below.

## Design

**Coverage is the quad, filled.** `draw_image` builds the unit square's four
corners through the transform and fills them with the non-zero rule over the
rectangle the draw already visits. Reusing `fill` buys the sixteen-sub-scanline
grid, exact 1/256 spans, integer accumulation, the active-edge sweep and the
cancellation contract — five properties a second edge walker would have to earn
again, and could then disagree with `fill` about.

The corners are **snapped to that 1/256 grid** before filling. An image
rectangle placed by a translation lands on a grid line far more often than not,
`fill` truncates a crossing at `(x * 256) as i64`, and `a + (e - t)` need not
equal `(a + e) - t` to the last bit — so an ulp below a grid line truncates to
the unit beneath it. `crop` found this: a cropped page must be the
sub-rectangle of the whole page *exactly*, and 240 pixels of `pclm-in.pdf`
disagreed until the snap went in. `round` is IEEE-exact, so this costs no
determinism.

**Sampling keeps one family across 1:1.** Hard samples are `Area` over the
pixel's own footprint, which above 1:1 is smaller than one sample: a pixel
inside a sample reads that sample alone, and only a pixel straddling two mixes
them. Hard pixels survive, a 1:1 blit stays byte-preserving, and the
scale-coherence in the table above becomes exact.

**A run of image draws accumulates before it composites.** `fragments.rs` holds
premultiplied colour and coverage, four bytes a pixel, added rather than
composited. Two strips each covering half a boundary pixel accumulate to one
whole pixel of their average — which is exactly what the doubled render
box-filters down to.

What ends a run: anything that paints, any change of alpha, blend mode, clip or
soft mask, a knockout group (11.4.5 gives each element its own shape), the page
itself, and a draw that would land on coverage the run already holds — adding
an overlap would show both pictures at once. What does *not* end a run is `q`,
`Q` or a form boundary, none of which paints. That distinction is the whole
difference between the feature working and not: real content brackets every
image in `q`/`Q`, and flushing there meant no run ever held two of anything.
The first build did exactly that, measured identical to no run at all, and cost
a 10x slowdown allocating a buffer per image.

Bounded at `MAX_IMAGE_RUN_PIXELS`, a quarter of `MAX_PAGE_PIXELS`: past it,
images composite one at a time and abutting ones conflate again, which is a
quality loss rather than a page that will not render.

## Where it got to

`tpdf probe --dpi 72`, qpdf corpus, the eight files the roadmap cites.
`pclm-out.pdf` exceeds the probe's own time gate on this machine and is not
comparable.

| file | `dpi` before | after | `rotate` | `crop` |
| --- | ---: | ---: | --- | --- |
| `large-inline-image.pdf` (+ `-ii-all`, `-ii-some`) | 3.2 % | **held** | held | held |
| `inline-images.pdf` | 4.4 % | 3.1 % | skipped | skipped |
| `inline-images-ii-all.pdf`, `-ii-some.pdf` | 4.4 % | 3.1 % | **broke 1.7 %** | held |
| `pclm-in.pdf` | 17.3 % | 15.9 % | held | held |

Three files that were failing now hold outright. `pclm-in.pdf` improved on its
baseline and kept `rotate` and `crop`. One regression is open and named: the
two `inline-images-ii-*` files break `rotate` at 1.7 % against a 1 % budget.
An anti-aliased image edge at a fractional offset does not transpose exactly,
where a quantised one did — the same reason the budget exists for glyphs, at a
larger amplitude because images have long straight edges. Whether that is a
budget to revisit or an artefact to remove is a decision, not an oversight.

`dpi` still breaks on the strip files. The residual is not edges and not
conflation; both were measured out. It is what remains of resampling itself,
and closing it would be a different item with different evidence.

**The `rotate` regression was attributed rather than guessed at**, by building
each half of the change on its own and re-measuring `inline-images-ii-some.pdf`:

| build | `rotate` | `dpi` |
| --- | ---: | ---: |
| magnification reverted to one nearest tap | 1.7 % | 3.1 % |
| edges hard again, everything else kept | **1.0 %** | 4.3 % |
| all of it | 1.7 % | 3.1 % |

So it is the anti-aliased edge, and nothing else: the file holds `rotate` at
exactly the budget with hard edges and fails at 1.7 % with soft ones, while
`dpi` moves the other way. Its images are not abutting — they are hundreds of
separately placed, 90°-rotated scans — so no amount of conflation work reaches
it. The whole-corpus shape of the same trade, `cargo xtask corpus-run --corpus
qpdf`: **`dpi` held on 577 of 582, up from 574**, and `rotate` on 477 of 479,
down from 479. Nothing else moved — 608 passed, 0 crashed, 487 of 487 rewrites
validate strictly.

Two coherent packages come out of that, and which one lands is a decision about
the budget rather than about the code:

- **Edges left hard.** The magnification fix alone still gains the three
  `large-inline-image` files, and nothing regresses. The roadmap's named defect
  stays.
- **Edges anti-aliased**, as built here. The same three gain, `pclm-in.pdf`
  improves to 15.9 % and `inline-images*` to 3.1 %, and `rotate`'s 1 % budget —
  set when no image edge was soft — no longer has headroom on two files.

## Milestones

| # | Deliverable | Exit criteria | Size |
|---|---|---|---|
| 1 | Coverage from the filled quad, snapped to the sweep grid | A half-covered edge is half the paint on either axis at all fifteen sixteenths; a corner is the product of its fractions; a 1:1 integer-aligned draw is byte-preserving | S |
| 2 | One filter family across 1:1 | A render and the box-filtered render at twice the scale agree at every ratio tried; `/Interpolate false` still reproduces whole samples | S |
| 3 | Runs, and what ends one | Two abutting strips leave 63 composited singly and 0 as a run; a shared boundary is the average of the two draws; runs survive `q`/`Q` | M |
| 4 | Fingerprints, docs, corpus | Four image-bearing fingerprints re-recorded and reproduced on `wasm32-wasip1`; `features/rasterizer.md` carries the trade; the numbers above recorded | S |

## Risks

| Risk | Mitigation |
|---|---|
| A run composited under the wrong clip paints through it | A run records the identity of the clip and soft mask in force and ends when either changes; every operation that swaps the canvas flushes outright |
| Fragments are added, so an overlap would show both pictures | Asked before every draw, over the band where the new draw meets what the run already covered rather than over either whole; a real overlap ends the run |
| Memory: a run buffers the canvas | Bounded to a quarter of `MAX_PAGE_PIXELS`, past which the old per-draw path runs unchanged |
| Anti-aliased image edges add transposition noise, which `rotate` measures | Not mitigated, and attributed rather than assumed: the table above builds each half separately and shows the edge is the whole of it. Left as a budget decision rather than absorbed quietly |
