# Phase 07 — Rasterizer

When this phase is done, `tinker-pdf-raster` is a complete, self-contained 2D
renderer: paths in, pixels out, with antialiased fills, strokes, clipping,
image sampling and SrcOver compositing — and zero PDF vocabulary anywhere in
its API (consistency ruling 8). It is shaped this way because this is the one
crate where a bug means wrong pixels in every rendered page, so it must be
testable against geometry it can be *proven* right about — analytic shapes,
coverage invariants, byte-identical goldens — without a PDF, a font, or a
content stream in sight. Everything is hand-rolled under MIT OR Apache-2.0 per
the project-wide decision; the determinism and tile contracts it must satisfy
are already law in [99-consistency](99-consistency.md) (rulings 4 and 5), and
this document is where they become mechanisms.

## Scope

- Path model: `MoveTo` / `LineTo` / `QuadTo` / `CubicTo` / `Close` over `f32`
  points. Quadratics are first-class, not up-converted — TrueType glyph
  outlines are quadratic and up-conversion adds float work and error for
  nothing.

  *Amended, August 2026 (gap [13](gaps/13-quadratic-path-verb.md)).* The verb
  set now matches: `Verb::QuadTo` exists, `Path::quad_to` builds one, the
  flattener has a quadratic arm and `show_glyph` emits quadratics rather than
  raising them. The cubic verb is spelled `CurveTo` rather than `CubicTo`, and
  each verb carries its own points instead of indexing a parallel array — a
  naming and layout difference, not a behavioural one.

  **`Point` is `f64`, not `f32`, and this plan is the thing that is out of
  date.** Every other piece of geometry in the tree is `f64`, the fill
  accumulator included, so narrowing the path model alone would add two
  conversions per point on the hottest path to save memory nothing is short
  of. Changing it is a decision with consequences for the accumulator and is
  explicitly out of gap 13's scope; until someone takes it, this line is
  aspiration and the code is fact.
- Bezier flattening with adaptive tolerance in device space.
- Fills under both PDF rules — nonzero winding (32000-1 §8.5.3.3.2) and
  even-odd (§8.5.3.3.3) — via scanline analytic-coverage antialiasing.
- Thin-stem dropout control: coverage quantization floor plus a hairline
  minimum-width rule for strokes.
- Stroking (§8.4.3.3–8.4.3.6): joins (miter with limit, round, bevel), caps
  (butt, round, projecting square), dash patterns with phase, the
  zero-length-subpath cap rules, all implemented as path expansion feeding the
  one fill pipeline.
- Clip stack as intersected u8 coverage masks, with an analytic
  rectangle fast path.
- Image sampling: nearest, bilinear, and area-averaging pyramid for downscale
  beyond 2:1, driven by an `interpolate` flag the render layer maps from the
  image dictionary's `/Interpolate` (Table 89).
- Compositing: SrcOver onto opaque and alpha-carrying targets, premultiplied
  internally; surface formats `Gray8`, `GrayA8`, `Rgb8`, `Rgba8`.
- The determinism mechanism: 24.8 fixed-point snap, integer-only inner loops,
  no platform libm, no FMA — bit-identical output across linux, windows,
  macos and wasm CI targets.
- The viewport/tile contract: integer surface origin applied after the
  fixed-point snap, so a tile is byte-equal to the full-page subregion.

## Non-goals

- The PDF transparency model — blend modes beyond Normal, soft masks,
  isolated/knockout groups — is composed by
  [08-rendering-device](08-rendering-device.md) from this crate's primitives
  (render to `Rgba8`/`GrayA8`, mask-weighted composite). This crate ships
  SrcOver and nothing else.
- Shading paints. Axial/radial/mesh shadings are decomposed by 08 into filled
  geometry with solid colors; if that proves to band, a vertex-interpolated
  triangle primitive is added *here* but scheduled *there*. Named as a seam,
  not silently assumed away.
- Color conversion and color management — `tinker-pdf-color`'s problem; this
  crate receives device-format pixels and composites them.
- Image decoding — [02-filters](02-filters.md). This crate receives decoded
  pixel planes.
- Glyph outline extraction, hinting, stem darkening —
  [05-fonts](05-fonts.md) and 08. Glyphs arrive as ordinary `Path`s; any
  emboldening is a geometry adjustment upstream of this crate.
- Text rendering modes, Type 3 glyph procedures —
  [06-content-and-text](06-content-and-text.md) and 08.
- SIMD and parallelism. Scalar correctness and determinism first; integer
  SIMD (which preserves bit-exactness) and tile-level threading are
  post-parity work in 08. Criterion benches exist from milestone 1 so the
  cost of this stance is measured, not guessed.

## Design

### Coordinate contract and API shape

Every draw call takes a path in its own coordinate space plus a 2×3 affine
transform to device space. The transform is applied *inside* the crate, for
two reasons: flattening tolerance is meaningful only in device pixels, and
PDF stroke semantics require the pen itself to be transformed (a non-uniform
CTM turns a round pen into an ellipse — §8.4.3.3). A caller that
pre-transformed its paths would have to redo both.

```rust
pub struct Point { pub x: f32, pub y: f32 }

pub enum Verb { MoveTo, LineTo, QuadTo, CubicTo, Close }

pub struct Path { /* verbs: Vec<Verb>, points: Vec<Point> — SoA, cheap to clone-share */ }

pub struct Transform { pub a: f32, pub b: f32, pub c: f32, pub d: f32, pub e: f32, pub f: f32 }

pub enum FillRule { NonZero, EvenOdd }

pub enum Join { Miter { limit: f32 }, Round, Bevel }
pub enum Cap { Butt, Round, Square }

pub struct StrokeStyle {
    pub width: f32,
    pub join: Join,
    pub cap: Cap,
    pub dash: Option<Dash>, // Dash { array: Vec<f32>, phase: f32 }
}

pub enum PixelFormat { Gray8, GrayA8, Rgb8, Rgba8 }

pub struct Surface<'a> {
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub origin: (i32, i32), // device coordinate of pixel (0,0) — the tile contract
    pub data: &'a mut [u8],
}

pub struct Rasterizer { /* reusable cell pool, edge and span scratch buffers */ }

impl Rasterizer {
    pub fn fill(&mut self, s: &mut Surface, path: &Path, t: Transform,
                rule: FillRule, paint: Paint, clip: &ClipStack);
    pub fn stroke(&mut self, s: &mut Surface, path: &Path, t: Transform,
                  style: &StrokeStyle, paint: Paint, clip: &ClipStack);
    pub fn draw_image(&mut self, s: &mut Surface, img: &ImageSource<'_>, t: Transform,
                      interpolate: bool, alpha: u8, clip: &ClipStack);
}

pub enum Paint { Solid([u8; 4]) } // premultiplied RGBA; Gray targets take the R channel
```

`Rasterizer` owns the scratch memory so a page render reuses one allocation
set; it is `Send` but not shared — one per worker, per the architecture's
concurrency model ([00-architecture](00-architecture.md)).

### Fill pipeline: analytic coverage, not supersampling

The pipeline is transform → flatten → snap to a 24.8 fixed-point grid →
cell accumulation → scanline sweep → composite. Flattening is adaptive
recursive subdivision with a device-space tolerance of 0.25 px (criterion:
maximum control-point deviation from the chord), depth-capped so a
pathological curve cannot recurse unboundedly.

*Amended, August 2026 (gap [13](gaps/13-quadratic-path-verb.md)).* Flattening
is adaptive but it is not recursive. Both subdividers take a fixed step count
from the control polygon's length — `ceil(sqrt(polygon / tolerance) * 1.5)`,
clamped to 512 — and sample the curve at evenly spaced parameters. The
recursive form was rejected under ruling 4, not on cost: a recursive
subdivider stops on a floating-point comparison, and a comparison that lands
differently on a 32-bit target than on a 64-bit one produces a different
number of segments rather than a slightly different one. A fixed count derived
from correctly-rounded arithmetic cannot do that, and the step cap replaces
the depth cap. The quadratic arm measures its polygon through the cubic it is
equal to, so the same curve gets the same count whichever verb carries it.

Coverage is computed analytically, FreeType-smooth style: each flattened edge
walks the pixel grid accumulating per-cell `(cover, area)` pairs in `i32` —
`cover` is the signed vertical extent crossing the cell, `area` the signed
area between the edge and the cell's left boundary. A per-scanline sweep then
integrates cover left-to-right and resolves winding to an 8-bit alpha:
nonzero clamps `|acc|` to full coverage; even-odd folds `acc` modulo two
windings into a triangle wave.

Supersampling was rejected deliberately. 4×4 supersampling yields 17 gray
levels at 16× the fill cost and still steps visibly on near-horizontal edges;
analytic coverage yields all 256 levels from exact polygon area at roughly 1×
geometry cost. It is also the easier determinism story: one integer
accumulation per cell in one defined order, rather than 16 subsamples whose
summation an optimizer might be tempted to reorder. This is the same family
of rasterizer MuPDF and FreeType use, which keeps our output in the visual
neighborhood Tinker's users already accept.

### Dropout control

Two rules, both cheap, both aimed at the same failure — features that shrink
below a pixel at zoomed-out scales:

- **Quantization floor.** A cell whose accumulated winding is nonzero never
  resolves to alpha 0; it floors at 1. Without this, a stem thinner than
  1/256 px vanishes entirely, and a visible-but-faint stem can be finished
  off by a subsequent u8 clip multiply.
- **Hairline minimum width.** A stroke whose device-space width falls below
  1 px is geometrically widened to exactly 1 px and its paint alpha scaled by
  the true width (fixed-point multiply), preserving apparent weight without
  gaps. Width 0 is special-cased per §8.4.3.3 — "thinnest device line" —
  as a 1 px hairline at full alpha, not alpha 0.

Fills get no artificial boost beyond the floor: analytic area coverage is
already dropout-free in the sense that matters, unlike monochrome
rasterization.

### Stroking as path expansion

`stroke` builds the stroke outline as an ordinary `Path` and hands it to the
nonzero fill pipeline. One coverage engine, one determinism story, one set of
bugs; joins and caps are just more segments, and self-intersecting expansions
(sharp turns, overlapping dashes) are exactly what the nonzero rule absorbs.
A dedicated stroke scanliner would be faster for hairlines, but it is a
second code path, and the hairline rule above already handles the case that
tempts one.

Mechanics: curves are flattened first (same tolerance machinery), then the
polyline is offset by ±width/2 — analytic cubic offsets are not
representable as cubics, so offsetting the flattened polyline is both simpler
and exact to the same tolerance. Joins per §8.4.3.5: miter with the limit
compared as miter-length/width, falling back to bevel when exceeded; round
joins and caps are tessellated by recursive chord bisection using vector
normalization only — the half-angle construction needs `sqrt` and nothing
else, which matters for determinism (below). Caps per §8.4.3.4.

Dashes per §8.4.3.6 operate on the measured flattened polyline: the pattern
restarts at each subpath with the phase re-applied, every dash end receives
the current cap, and a zero-length dash segment with round caps produces the
classic dot. Degenerate subpaths (a single point, or all points coincident):
round caps paint a filled circle of diameter `width`; butt caps paint
nothing (spec-directed); projecting square is undefined in the spec — we
paint an axis-aligned square, matching observed Acrobat/Ghostscript
behavior, and document the choice.

### Clip stack

```rust
pub struct ClipStack { /* Vec of entries: Rect(analytic) | Mask { bbox, Vec<u8> } */ }

impl ClipStack {
    pub fn push_rect(&mut self, rect: [f32; 4], t: Transform);
    pub fn push_path(&mut self, r: &mut Rasterizer, path: &Path, t: Transform, rule: FillRule);
    pub fn pop(&mut self);
}
```

A path clip (§8.5.4) rasterizes through the same fill pipeline into a u8
coverage mask over the clip's device bounding box and intersects with the
current top by per-pixel multiply, `(a * b + 127) / 255`. Axis-aligned
rectangle clips — the overwhelmingly common case — stay analytic: integer
interior plus fractional edge coverage applied as a span modifier, no mask
materialized until a non-rectangular clip is pushed above them. Every draw
multiplies its source coverage by the effective clip coverage.

**u8, not u16.** A full-page mask at 300 dpi A4 is ~8.7 MB; u16 doubles that
for precision below what an 8-bit output can show. Chained u8 multiplies with
round-half-up lose at most half a level per nesting depth, and PDF clip
nesting is shallow in practice. The mask element type is a private alias; if
oracle comparisons at Checkpoint B show banding in deeply nested clips, the
escape hatch is a one-line widening, and that trigger is recorded in Risks.

### Image sampling

`ImageSource` is a borrowed decoded pixel buffer in one of the four surface
formats, premultiplied on ingest. The sampling policy, decided and
documented:

| Condition | Filter |
| --- | --- |
| Scale ≥ 1 per axis, `interpolate` true | Bilinear |
| Scale ≥ 1 per axis, `interpolate` false | Nearest |
| Downscale up to 2:1 | Bilinear |
| Downscale beyond 2:1 | Box-filter pyramid to within 2:1, then bilinear |

The debatable cell is nearest-at-≥1× for `interpolate` false. Decision:
nearest. Table 89 defines `/Interpolate` as opt-*in* smoothing for magnified
images; false means the author wanted hard pixels (barcodes, screenshots,
upscaled 1-bit masks), and nearest keeps a 1:1 blit byte-preserving — a
property the tile contract tests lean on. Some viewers smooth everything;
`pdfcmp`'s perceptual gate absorbs that divergence, and 08 can add a
quality override without touching this crate.

Downscale beyond 2:1 must not use bilinear alone: four taps regardless of
footprint skips texels and produces moiré that diverges visibly from
MuPDF's area-averaged output — a parity problem, not a taste problem. The
pyramid is repeated exact 2× area-averaging (integer, round-half-up), level
chosen per axis from the transform's column-vector lengths (`sqrt` only),
then bilinear for the residual. Rotated and sheared placements walk
destination pixels through the inverse transform with a 16.16 fixed-point
DDA — the inner loop is integer like every other inner loop here. Pyramid
levels are returned to the caller for caching; 08 owns image caching policy,
this crate stays stateless between calls.

### Compositing

Internally everything is premultiplied alpha, and the only operator is
SrcOver: `d' = s + d * (255 - a_s) / 255`, integer with `(x * y + 127) / 255`
rounding. `Gray8` and `Rgb8` are opaque targets (no alpha stored, destination
alpha assumed 255 — these are Tinker's actual output formats, per
`render_pages.rs` asserting 3 and 1 components); `GrayA8` and `Rgba8` carry
premultiplied alpha and exist so 08 can compose transparency groups. Any
un-premultiplication for straight-alpha consumers happens in 08, once, at
the edge.

### Determinism

Ruling 4 made bit-identical cross-target output a contract; this is the
mechanism. The rules, in force from the first milestone:

- All coverage and compositing arithmetic is integer: 24.8 snap, `i32`
  cover/area cells (`i64` where products can overflow), integer span and
  pixel loops. There is no float in any per-pixel or per-cell path.
- Float appears only in setup — transform application, flattening, stroke
  offsetting, dash measurement — and is restricted to `+ - * /` and `sqrt`,
  all of which IEEE 754 requires to be correctly rounded and which therefore
  produce identical bits on x86-64, aarch64 and wasm32.
- `mul_add` is banned (clippy `disallowed_methods`): FMA contraction is the
  one way an optimizer legally changes float results. Rust does not contract
  on its own, but the ban makes the guarantee ours rather than the
  compiler's.
- No platform libm, anywhere: `sin`/`cos`/`atan2` route to OS libraries with
  differing last bits. The crate needs no transcendentals — round geometry
  is built by chord bisection (`sqrt` only), pyramid levels from vector
  lengths (`sqrt` only).
- The golden gate: a synthetic scene suite (fills under both rules, every
  join/cap/dash combination, nested clips, image placements at magnify /
  shrink / rotate, hairlines) rendered to all four formats, SHA-256 of the
  raw bytes committed. The determinism CI job from
  [00-architecture](00-architecture.md) compares hashes across
  linux/windows/macos/wasm on every PR; any divergence is a build stopper,
  never a tolerance bump.

### The tile contract

Ruling 5: a tile renders through the same code path as the full page, with a
translated viewport — never a second implementation. MuPDF's bindings are the
cautionary tale (limitation #8 in Tinker's `mupdf-limitations.md`: whole-page
`to_pixmap` versus a hand-assembled pixmap/device/clear dance for tiles —
two paths, two behaviors).

The mechanism is `Surface.origin`. All geometry is computed in absolute
device coordinates through the fixed-point snap; the integer origin is
subtracted in cell space, after snapping, and cells outside the surface are
discarded. Because the offset is an integer applied downstream of every
rounding decision, a tile and a full page perform *identical arithmetic* for
their overlapping pixels — byte equality is a consequence of the design, not
a tuning target. Tinker's
`crates/tinker-core/tests/render_pages.rs::a_tile_matches_the_same_region_of_the_full_page`
pins exactly this, row-by-row against the full-page subregion, and that test
(ported verbatim under ruling 12) is a permanent parity gate from this phase
forward. A proptest generalizes it here: random scene, random integer
viewport, byte-equal subregion.

### Errors and leniency

Drawing never fails. `RasterError` exists only for caller-contract violations
surfaced at construction time (stride/format mismatch, zero-sized surface).
Geometry is sanitized, not rejected: segments with non-finite points are
dropped, coordinates saturate to the 24.8 range (±8.3M px), a singular
transform draws nothing — all documented, all silent, because a leaf crate
has no warning channel and the render layer can pre-validate when it wants
provenance (ruling 10 applies at *its* level). Panics are bugs; ruling 1 is
enforced by the fuzz targets below.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Path model, flattener, cell accumulator, nonzero + even-odd fills onto `Gray8`; golden harness and 4-target hash gate wired from day one | Circle (cubic-approximated) and rect fills within 1/255 per-pixel of closed-form area coverage; even-odd star fixture correct; first golden hashes byte-equal on linux/windows/macos/wasm | M |
| 2 | Surface formats, premultiplied SrcOver, `Paint`, viewport origin | Proptest: random scene × random integer viewport → tile bytes equal full-render subregion; SrcOver algebraic identities (opaque src replaces, transparent src preserves) hold exactly in u8 | S |
| 3 | Stroker: offsets, joins with miter limit, caps, dashes with phase, degenerate-subpath rules, hairline minimum width | Stroked-square vertices under each join within one snap unit of analytic positions; dash segment counts exact on measured fixtures; round-cap dot and dotted-line fixtures match reference geometry; 0.3 px hairline renders unbroken at proportional alpha | M |
| 4 | Clip stack: analytic rect fast path, mask materialization, intersection | Rect-as-path clip byte-equal to analytic rect clip; nested rect∩circle∩star equals the single-mask product reference; proptest: clip-then-draw equals draw-then-multiply | S |
| 5 | Image sampling: nearest, bilinear, box pyramid, policy matrix as documented | 1:1 nearest blit byte-preserving; 8× checkerboard downscale equals reference box filter exactly; rotated-placement golden stable across targets; policy table published in rustdoc | M |
| 6 | Hardening: fuzz targets (arbitrary verbs/points/transforms/styles; arbitrary clip programs), saturation proptests, determinism gate becomes a required check | Fuzzers panic-free and OOB-free over the accumulated corpus plus a soak budget in CI; proptest invariants (coverage ≤ 255 everywhere; shape + rect-complement fills sum to full coverage per pixel; integer translation shifts output exactly) hold; full golden suite bit-exact on all four targets | S |

Phase total sits in the L band; milestones 1 and 3 carry the substance,
2 and 4 are deliberately small because their design is fixed here.

## Dependencies

- **Upstream:** only [00-architecture](00-architecture.md) — the crate
  skeleton, clippy/deny/fuzz scaffolding, and the determinism CI harness that
  this phase's golden gate plugs into. No workspace crate dependencies:
  `tinker-pdf-raster` is a leaf (ruling 8), and stays one. Dev/build tooling
  (proptest, criterion, cargo-fuzz) is exempt from the hand-rolled rule by
  the project decision.
- **Parallelism:** starts alongside [02-filters](02-filters.md),
  [03-encryption](03-encryption.md) and [05-fonts](05-fonts.md); shares
  nothing with them until 08 joins the seams.
- **Downstream:** [08-rendering-device](08-rendering-device.md) is the sole
  consumer — the rasterizing `Device` drives every call in this crate's API,
  including glyph rendering (font outlines from [05-fonts](05-fonts.md)
  arrive as `Path`s via 08). Not on the Checkpoint A path at all — that is
  the point of the `content → Device` seam. On the Checkpoint B critical
  path; see [PLAN.md](../PLAN.md) for the checkpoint definitions.

## Risks

| Risk | Mitigation |
| --- | --- |
| Cross-target bit divergence appears despite the rules (autovectorized float summation, wasm rounding corner) | Variance sources are removed structurally, not chased: integer inner loops, correctly-rounded-only float ops, `mul_add` ban. The 4-target hash gate runs from milestone 1 on trivial scenes, so a divergence surfaces against ten lines of geometry, not ten thousand |
| Stroker wrong on hard geometry: cusps, near-parallel offsets, self-overlapping dashes | Flatten-then-offset sidesteps analytic offset instability; nonzero fill absorbs self-intersection by construction; cusp-adjacent cubics get forced subdivision at extrema; milestone 3's analytic fixtures plus the milestone 6 fuzzer cover the rest |
| Scalar performance lags MuPDF enough to hurt interactive use | Criterion benches from milestone 1 keep the gap measured; budget is set at Checkpoint B, not here; integer SIMD preserves bit-exactness and is the known lever, tile parallelism in 08 is the second — neither requires design changes in this crate |
| u8 clip masks band under deep nesting | Bounded at ≤0.5 level per depth; mask element type is a private alias, widened to u16 the day a Checkpoint B oracle comparison shows it — a recorded trigger, not a re-litigation |
| Hairline and `interpolate=false` policies diverge visibly from oracle renderers | Both are documented decisions with reasons; `pdfcmp` perceptual thresholds absorb them, and each has a named revisit path in 08 that does not touch this crate's contract |
| Pyramid levels for huge images cost memory in tight environments (wasm) | Levels are built on demand, per axis, only beyond 2:1, and handed to the caller — 08 owns the cache and its budget, so wasm policy is set where memory policy lives |
| The wasm golden run needs a host runner in CI | Node/wasmtime are dev tooling, exempt from the hand-rolled rule; the harness is proven in phase 00 before this phase has pixels to protect |

---

Rulings 4, 5 and 8 in [99-consistency](99-consistency.md) bind this plan;
[08-rendering-device](08-rendering-device.md) consumes it;
[14-testing-and-corpora](14-testing-and-corpora.md) owns the corpus and oracle
doctrine the exit criteria lean on. Master plan: [PLAN.md](../PLAN.md).
