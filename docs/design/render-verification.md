# Render verification without a second engine

This design makes a page checkable against something other than this
engine's own opinion of it. Without it, the test suite proves the engine
agrees with itself, the corpus run proves a bitmap came back, and nothing
proves the bitmap is *right* ([verification.md](../verification.md)).

Ruling 13 rules out rendering the same page with another engine and diffing
the two. It is worth being blunt about the trade: a second engine is a
genuinely independent opinion, and nothing here is. The mechanisms below are
ordered by how far each gets from the engine's own reasoning, and the last
section names what none of them reach.

## Scope

- **Analytic fixtures.** Pages whose correct raster is computable in closed
  form, with the expected value produced by an independent function inside
  the test rather than by the renderer.
- **Metamorphic probes** over the real corpus: properties that must hold
  between two renders of the same file, checked across thousands of
  documents nobody here authored.
- **Differential in-tree pairs**: two code paths that must produce the same
  pixels from the same source bytes.
- **Reviewed goldens**: small committed rasters per operator family, read
  once by a named person against the clause, with the review recorded.
- A ratchet for the metamorphic rows, on `corpus/ratchet.json`'s discipline:
  counts, never rates, compared by `ratchet::holds`.

## Non-goals

- **A second renderer, in any form.** Ruling 13. Not vendored, not linked,
  not a subprocess, and not "only to record the baseline once".
- **Correctness adjudication of arbitrary content.** No mechanism here can
  say whether an arbitrary real-world page is drawn the way the world draws
  it. That claim is not made, and the gap section says so.
- **Committed full-page goldens over the corpus.** Hundreds of kilobytes of
  binary per page that no reviewer can assess, for pages nobody chose. The
  goldens here are small, few and deliberate.

## Design

### 1. Analytic fixtures — the only tier that answers to mathematics

The strongest substitute available, and the one furthest from the engine's
own reasoning: the expected raster is computed by a second function, written
from the geometry rather than from the code, and the two are compared byte
for byte. Where a closed form exists this is *better* evidence than a second
engine, because it has no opinion to be wrong about.

What has a closed form:

- **Coverage.** An axis-aligned rectangle at integer coordinates is exactly
  covered; at half-integer coordinates each edge pixel is exactly half. A
  half-plane of rational slope has per-pixel coverage computable by area.
  These pin the fixed-point accumulator ruling 4 rests on, at the level of
  `tinker-pdf-raster`, where the arithmetic actually lives.
- **Fill rules.** For two overlapping subpaths, nonzero minus even-odd is
  exactly the intersection area — two independently computed numbers that
  must differ by a third independently computed number.
- **Strokes.** A straight segment's stroked area is length times width plus
  a cap term that is zero, one square of the width, or a disc of it, by cap
  style. `sqrt` is correctly rounded and permitted on pixel paths (ruling
  4's stated boundary), so the expectation is exact.
- **Shadings.** Type 2 and Type 3 (8.7.4.5.3–4) are per-pixel formulas.
  Every pixel of an axial or radial shading is computable directly from the
  parametric equation and the function, with no reference to how the shading
  code walks the page.
- **Blend modes.** ISO 32000-1 11.3.5 publishes each separable mode as an
  arithmetic expression on backdrop and source. `tests/blend_modes.rs`
  exercises them, and the analytic tier compares them per pixel in closed
  form over a constructed backdrop.
- **Image sampling.** A two-by-two checker scaled by an integer factor has
  an exactly known result under nearest-neighbour and known bounds under
  interpolation.

Where they live: coverage, fill-rule and stroke cases in
`tinker-pdf-raster`'s `tests/analytic_coverage.rs`, beside the arithmetic
they check — among them `a_shallow_edge_keeps_the_slope_the_line_states`,
which answers to the line's own equation rather than to how an edge is
stored; operator-level cases at the facade in
`crates/tinker-pdf/tests/render_analytic.rs`, since those need a content
stream — both shadings compared per pixel over the whole page, the twelve
separable modes, integer image scaling — each page held to a least-ink floor
there. The pages are built by one function each in `tests/render_support/`,
and the axial, radial, image and blend-grid pages are fingerprints in
`determinism.rs`, the test the wasm and Linux legs run on their own, so the
document the equation adjudicates is the document the hash covers.
`blend_grid_page` lays the twelve modes out as one picture, held to the
clause cell by cell, because the per-mode fixture builds many documents and
samples one pixel of each, which cannot be enrolled as a fingerprint.

### 2. Metamorphic probes — breadth, over documents nobody here wrote

Analytic fixtures are exact but narrow: they cover pages built to be
provable. The corpus is the opposite — real files with no known answer.
What can still be asserted there is a *relation between two renders* of the
same file, which needs no ground truth at all:

- **Rotation.** A page with `/Rotate 90` rendered directly must equal the
  `/Rotate 0` render under a trivial pixel transposition. This catches every
  place rotation is applied inconsistently between geometry, clipping, text
  and images.
- **Cropping.** A render of a translated `CropBox` must equal the
  corresponding sub-rectangle of the full render. This is ruling 5's
  tile-equality property, generalised from tiles to the page box and applied
  corpus-wide rather than to fixtures. The property it generalises is pinned
  in-tree, on fixtures and at tile sizes that do not divide the page:
  `crates/tinker-pdf/tests/render_regions.rs`. The two are worth keeping
  apart — this one is breadth over documents nobody here wrote and no known
  answer, that one is a byte-equality assertion with one named exception
  ([rulings](../rulings.md)).
- **Resolution coherence.** A render at twice the scale, box-filtered down
  by two, must agree with the direct render within a budget measured and
  ratcheted rather than guessed. Sampling-grid and rounding bugs move this;
  a wrong colour does not.

These run as `tpdf probe` modes — `meta rotate`, `meta crop` and
`meta dpi`, each checked in-process on the first page so no image leaves the
child — driven by `xtask corpus-run`, and are recorded as rows in
`corpus/ratchet.json`: per-corpus `compared` and `held` counts, never rates,
compared by `ratchet::holds` exactly as the pass rate is.

**What they cannot catch, stated because it is why they are second and not
first:** any defect that commutes with the transformation. A colour
converted wrongly is converted equally wrongly at both resolutions and at
both rotations, and every one of these probes stays green.

### 3. Differential in-tree pairs

Two paths, one source, identical pixels required. `tests/inline_images.rs`
does this for inline images against image XObjects, and
`tests/render_differential.rs` for a Type 3 glyph against the same path, a
tiling pattern against its unrolled cells, a form XObject against its
inlined operators, and a shading pattern against `sh` in the same region.
Each asserts the whole bitmap with no budget, and each half carries an ink
floor, because two blank pages agree perfectly. The tiling pair's step
differs from its cell and the shading pattern's `/Matrix` is not the
identity, since either would collapse the two readings the pair exists to
part.

Weakest of the four, and honestly so: both sides cross the same rasterizer,
so only path-specific bugs are reachable. It is cheap, and it is the only
tier that covers the *plumbing* between two features.

### 4. Reviewed goldens

Small committed rasters, one per operator family, read once by a named
person against the clause they implement, with the reviewer, the date and
the clause recorded in the fixture's header
(`crates/tinker-pdf/tests/render_goldens.rs`, the rasters in
`crates/tinker-pdf/tests/goldens/`). This is the tier whose value depends
entirely on the review actually having happened, so the record of the
review is part of the fixture rather than part of a commit message.

They catch gross misreadings at review time and every regression afterwards.
They do not catch what the reviewer's reading shares with the implementer's,
which here is everything, because they are the same people.

## The gap, named

**No mechanism in this document proves that a rendered page agrees with the
world's reading of it.** A consistent misreading of ISO 32000 that survives
one human review draws the wrong picture on every target, deterministically,
forever, and nothing in this suite fires.

That is the claim a second engine made and this design does not. It is
recorded in [verification.md](../verification.md) in its own voice rather
than absorbed, because a known gap is manageable and a suite that has
quietly stopped proving something is not.

## Milestones

1. Analytic coverage and fill-rule fixtures in `tinker-pdf-raster`
   (`tests/analytic_coverage.rs`): expected coverage computed from the
   sampling grid by an in-test function, byte-equal.
2. Analytic operator fixtures at the facade (`tests/render_analytic.rs`):
   axial and radial shadings, separable blend modes, integer image scaling,
   their pages fingerprinted in `determinism.rs`.
3. Metamorphic probe modes in `tpdf probe`: `meta rotate`, `meta crop` and
   `meta dpi`.
4. `corpus-run` rows and ratchet entries for the three relations: per-corpus
   `compared` and `held`, every budget recorded with the measurement that set
   it.
5. Differential in-tree pairs (`tests/render_differential.rs`).
6. Reviewed goldens per operator family. Exit: each golden's header names
   the reviewer, the date and the clause; a golden missing any of the three
   fails a test that reads the headers, in the style of `bounds_ledger.rs`.

## Dependencies

- `xtask` plumbing: `ratchet::holds`, `report.rs` serialization,
  `runner.rs`'s per-file timeout discipline, `corpus.rs::pdfs_under`.
- The fetched corpora, which ruling 13 keeps: they are inputs, not
  adjudicators.
- `crates/tinker-pdf/tests/determinism.rs` for the fingerprint mechanism the
  analytic fixtures reuse.

## Risks

Each relation's budget is recorded beside the measurement that set it, in
`tools/tpdf/src/main.rs`: `dpi` carries `DPI_BUDGET` (2 %); `rotate` carries
`ROTATE_BUDGET` (10 %), above the measured cost of a page of anti-aliased
text and below the measured defects, and the same measurement is judged
again at 3 % as `rotate-tight`, which decides nothing and is ratcheted on
its own; `crop` carries none, because a crop moves the page box and nothing
else.

| Risk | Mitigation |
| --- | --- |
| Analytic fixtures only cover pages simple enough to have closed forms, and real pages are not | That is why the metamorphic probes exist and why the corpus stays. Neither replaces the other; this design carries both |
| A metamorphic relation is satisfied by a bug that commutes with it, and the green row reads as coverage | Stated in the design and in the ratchet's own note, the way `ratchet.json`'s note states its font-policy limit |
| A budget is set loose enough to catch nothing | It is recorded with the measurement that set it, and tightened by diffed edits with evidence — never widened to make a run green |
| Goldens are reviewed once, by the person who wrote the code they check | Recorded rather than resolved: the reviewer and the date are in the fixture, so the weight of the review is visible instead of assumed |
| The whole design is the engine checking itself, and agrees with itself for the same reason it is wrong | The analytic tier answers to arithmetic rather than to the engine, and the corpus answers to producers rather than to this project — but the gap section is the honest statement, and it stays in `verification.md` |
