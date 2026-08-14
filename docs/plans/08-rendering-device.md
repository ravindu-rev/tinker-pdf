# Phase 08 — Rendering device

When this phase is done, `Page::render` produces pixels: `tinker-pdf-render`
implements the rasterizing `Device` over the [07-rasterizer](07-rasterizer.md)
geometry core, and `tinker-pdf-color` supplies every color conversion and the
PDF function interpreter that colors, shadings and tint transforms all run on.
This is the phase Checkpoint B waits for and the master plan's declared long
pole. It is shaped as a `Device` implementation because ruling 7 of
[99-consistency](99-consistency.md) makes the interpreter seam the only seam —
the text path built in [06-content-and-text](06-content-and-text.md) never
links any of this — and color lives in its own leaf crate under ruling 8 so
the math is fuzzable and testable with zero PDF vocabulary in sight. The exit
bar is not "draws something": it is Tinker's `render_pages.rs` ported and
passing exactly, plus corpus-wide perceptual parity against the MuPDF oracle,
ratcheted in CI.

## Scope

Color (`tinker-pdf-color`):

- DeviceGray, DeviceRGB, DeviceCMYK (8.6.4) with fixed, deterministic
  transforms to the render space.
- Indexed (8.6.6.3) with palette pre-conversion at bind time.
- ICCBased (8.6.5.5) resolved to `/Alternate`, else to Device{Gray,RGB,CMYK}
  by `/N` — the profile bytes are validated for shape but never interpreted.
- CalGray (8.6.5.2), CalRGB (8.6.5.3), Lab (8.6.5.4) as documented
  approximations: gamma + matrix to XYZ (D50), then to sRGB primaries.
- Separation (8.6.6.4) and DeviceN (8.6.6.5) through tint-transform
  functions, including the `/All` colorant special case.
- The function interpreter (7.10): Type 0 sampled with multilinear
  interpolation and `/BitsPerSample` 1–32 (7.10.2), Type 2 exponential
  (7.10.3), Type 3 stitching (7.10.4), Type 4 PostScript calculator (7.10.5)
  on our own tiny stack interpreter — the hand-rolled rule applies here as
  everywhere.

Device (`tinker-pdf-render`):

- Fills and strokes of the interpreter's path ops through 07, with the full
  graphics-state stack, dash/join/cap parameters, and clip stack.
- Images: Image XObjects and inline images (8.9.7), every wave-1/wave-2
  filter from [02-filters](02-filters.md), `/Decode` arrays (8.9.5.2),
  `/SMask` soft-mask images (11.6.5.3), color-key masking (8.9.6.4), and
  `/ImageMask` stenciling (8.9.6.2).
- Shadings 1–3 — function-based, axial, radial with `/Extend` and the
  degenerate-cone cases (8.7.4.5.2–4) — day one; mesh types 4–7 ship as a
  `Capability` flag with the ruling-3 corpus hit-rate gate deciding if and
  when they are implemented.
- Patterns (8.7.3): tiling patterns, colored and uncolored paint types, with
  correct pattern-space anchoring, `/XStep`/`/YStep` gaps and overlaps;
  shading patterns.
- Text render modes 0–7 (9.3.6), including the clip modes 4–7 accumulating
  glyph coverage into the clip stack; Type 3 glyphs via content recursion
  (9.6.5) with [05-fonts](05-fonts.md) and 06.
- Transparency (clause 11), staged as its own milestone inside this phase:
  constant alpha `/CA`/`/ca`, the separable blend modes day one and the
  non-separable HSL set (11.3.5.3) following, soft masks (luminosity and
  alpha, 11.6.5), transparency group XObjects (11.6.6) with isolation
  (11.4.4) and knockout (11.4.5), and the page group.
- Annotation appearance streams (12.5.5): `/AP` Normal appearance with `/AS`
  state selection, behind a toggle matching Tinker's `include_annotations`.
- Optional content (8.11): OCG/OCMD default visibility from
  `/OCProperties` `/D`, including `/VE` visibility expressions (8.11.2.3).
- The public render contract: `RenderOptions { transform, clip, format,
  annotations, background, cancel }` → `Bitmap { width, height, format,
  stride, data, warnings }`, the outward-rounding guarantee
  (A4 @ 150 dpi = 1240×1755), cooperative cancellation, and the ruling-2
  degrade policy.

## Non-goals

- **Geometry.** Edge walking, coverage accumulation, spans, the stroker —
  all [07-rasterizer](07-rasterizer.md). This phase consumes fills, it does
  not implement them.
- **The interpreter and the `Device` trait.** Both live in
  [06-content-and-text](06-content-and-text.md); this phase implements the
  trait, never extends it unilaterally (ruling 7).
- **Glyph outlines and the glyph raster cache.** [05-fonts](05-fonts.md)
  wave 2 delivers coverage masks; this phase places and composites them.
- **Full ICC.** A named later capability, gated on evidence per ruling 3.
  The v1 color story is the alternate/`/N` fallback above; the resulting
  shifts against MuPDF are absorbed when Tinker's goldens are regenerated at
  integration — a locked decision, restated here, not reopened.
- **Mesh shadings 4–7 as implementations.** The capability flag, placeholder
  and warning ship day one; the decoders are built only if the hit-rate
  report says real documents need them.
- **A layer-toggle API.** Default OC visibility only; per-render layer
  overrides are a later facade addition once a consumer exists.
- **Appearance generation.** Annotations without `/AP` render nothing here;
  synthesizing appearances is [11-forms](11-forms.md).
- **Overprint, halftones, transfer functions.** Prepress state is parsed and
  deliberately not simulated — see the ExtGState table in Design.
- **A display list.** Tinker caches MuPDF display lists because
  re-interpretation was the expensive step and MuPDF renders are not
  reproducible enough to memoize any other way. Our determinism contract
  (ruling 4) makes re-renders byte-identical by construction, so v1 renders
  from the content stream every time and Tinker caches bitmaps. If
  integration profiling shows interpretation itself is the bottleneck, a
  recording device is a compatible later addition — the `Device` seam
  already permits it.

## Design

### Crate boundaries

`tinker-pdf-color` is a leaf: plain structs and sample buffers in, sample
buffers out. It never sees a COS object. The binder in `tinker-pdf-content`
translates color-space arrays, function dictionaries and tint transforms into
the color crate's value types — the same division [02-filters](02-filters.md)
uses for `/DecodeParms`. The function interpreter lives in the color crate
rather than the content crate because its two consumers (tint transforms,
shadings) are both color machinery, and because a Type 4 program is bytes —
tokenizing it needs no PDF context, which keeps it independently fuzzable.

```rust
pub enum ColorSpace {
    DeviceGray, DeviceRgb, DeviceCmyk,
    CalGray { gamma: f32, wp: [f32; 3] },
    CalRgb { gamma: [f32; 3], matrix: [f32; 9], wp: [f32; 3] },
    Lab { range: [f32; 4], wp: [f32; 3] },
    Indexed { base: Box<ColorSpace>, palette: Vec<u8>, hival: u8 },
    Separation { alt: Box<ColorSpace>, tint: Function },
    DeviceN { n: u8, alt: Box<ColorSpace>, tint: Function },
}

impl ColorSpace {
    /// Convert `n_components` interleaved samples to the render space.
    /// Deterministic on every target; no platform libm.
    pub fn to_render(&self, src: &[f32], dst: &mut [RenderColor]);
}
```

### The function interpreter

```rust
pub enum Function {
    Sampled { domain: Vec<[f32; 2]>, range: Vec<[f32; 2]>, size: Vec<u32>,
              bps: u8, encode: Vec<[f32; 2]>, decode: Vec<[f32; 2]>,
              samples: Vec<u8> },
    Exponential { c0: Vec<f32>, c1: Vec<f32>, n: f32 },
    Stitching { parts: Vec<Function>, bounds: Vec<f32>, encode: Vec<[f32; 2]> },
    Calculator { ops: Vec<CalcOp> }, // flat array, jump offsets for if/ifelse
}

impl Function {
    pub fn eval(&self, input: &[f32], output: &mut [f32]) -> Result<(), FnError>;
}
```

Type 0 interpolates multilinearly over the 2^m hypercube corners; `m` is
capped (budget, not conformance — real functions are m ≤ 2) and out-of-domain
inputs clamp per spec. Type 4 is parsed once into a flat op array so
evaluation is a loop, not recursion; the operand stack is capped at the
spec's 100 entries, there is an instruction budget per eval, and the
transcendentals (`sin`/`cos` in degrees, `exp`, `ln`, `atan` with the
PostScript quadrant convention) are our own implementations with committed
test vectors, per the determinism policy in
[00-architecture](00-architecture.md). Division by zero, stack underflow and
type errors clamp the output to `/Range` and warn — a broken tint transform
is a wrong color, never a lost page. `cargo-fuzz` targets cover the Type 4
tokenizer and all four evaluators from their first commit.

Per-sample evaluation of a Type 4 program is ruinously slow, so scalar-input
functions (tints, shading color ramps) are sampled once into a 1024-entry
LUT in render-space color and evaluated by index + lerp. A unit test pins the
LUT's worst-case error below `pdfcmp`'s materiality threshold, so the
optimization is provably invisible.

### Color pipeline

The render space is 8-bit sRGB-primaries RGB (or gray for `Gray8` output).
Every source converges to it at paint time: gray replicates, CMYK uses the
fixed `(1−c)·(1−k)` conversion, Lab and the Cal spaces go through XYZ with
our own deterministic `pow`. Indexed palettes are converted once at bind
time. ICCBased picks its fallback at bind time and records nothing unless
the profile is malformed, in which case the `/N` fallback applies with a
warning (ruling 10). All conversion inner loops are integer or fixed-point;
CI's determinism job compares bitmap hashes across linux/windows/macos/wasm
from the first milestone (ruling 4).

### The device, state, clips

`RenderDevice` implements `Device` from 06 over a single target buffer plus
a stack of transparency-group buffers (below). Graphics state is a plain
stack; clips are 8-bit coverage masks stored as bbox + buffer, produced by
the 07 filler, and intersected multiplicatively and lazily. One mask
representation handles path clips, even-odd clips, text clips and stencil
edges identically, with AA for free; the cost is memory proportional to the
clip bbox, bounded by page size and guarded by an allocation budget.
Ruling 5 binds the tile path: a `clip` render is the same pipeline with a
translated viewport, never a second implementation — Tinker's
tile-equals-subregion test is the permanent proof.

### Images

The interpreter delivers an image as decoded samples (via 02) plus the
parsed `/Decode` array and bound color space; the device fuses decode-array
remapping and color conversion into one pass per sampled pixel rather than
remapping the whole buffer first — the same work, half the memory traffic.
Painting inverse-maps each device pixel in the image's device bbox (clipped)
through the CTM into image space: bilinear when downscaling or when
`/Interpolate` is true, nearest otherwise; `/ImageMask` stencils sample with
box-filter coverage so their edges anti-alias like paths and paint the
current fill color, honoring the `[1 0]` decode inversion. `/SMask` images
are sampled independently through the same inverse map — their grid need not
match the base image's — and multiply into source alpha; `/Matte` is rare
enough to record-and-warn. Color-key masking range-tests raw samples before
decode, per 8.9.6.4.

Long decodes run in strips with the cancel token checked at each strip
boundary via a callback handed to the 02 decoders. Decoded images land in a
per-document byte-budgeted LRU (default 32 MiB native, 8 MiB wasm) keyed by
object ref, so repeated `Do` of one XObject and pattern replays do not
re-decode. A JBIG2/JPX/arith-JPEG image degrades exactly as ruling 2
prescribes: flat mid-gray in the image's device rect,
`Warning::Degraded { capability, object }`, page succeeds.

### Shadings

Axial and radial shadings evaluate per pixel: project onto the axis (type 2)
or solve the two-circle quadratic for the largest valid `s` (type 3,
8.7.4.5.4, including `r0 = r1`, concentric and degenerate-cone cases), map
through `/Domain`, then index the pre-sampled color LUT. `/Extend` clamps
`t` outside the domain; `/Background` fills unpainted areas when present;
`/BBox` intersects the clip; `/AntiAlias` is ignored because clips are
anti-aliased anyway. Type 1 inverse-maps each pixel through the shading
matrix and evaluates the 2-in function (LUT-free — it is the rare case).
The `sh` operator fills the current clip; shading patterns fill through the
painted shape's coverage. Mesh types 4–7 hit the capability path.

### Patterns

Tiling patterns re-enter the interpreter with the pattern's content stream
and resources — the same pipeline, budgeted. The pattern matrix anchors to
the default space of the pattern's parent content stream, not to the CTM at
paint time; getting this wrong is the classic tiling bug and gets a dedicated
fixture.

*Amended, August 2026, on building it (gap [09](gaps/09-tiling-patterns.md)).*
Three things in the paragraph above were wrong or unaffordable:

- **There is one replay strategy, not two.** The plan chose per paint:
  offscreen-and-blit when the composed matrix is axis-aligned and the steps
  land on device integers, per-cell replay otherwise. The second branch is not
  affordable at all — every replayed cell needs its own bounding-box clip, and
  a clip goes through `save_state`, which clones a page-sized mask. A 10 pt
  hatch over A4 is roughly 4 800 cells and several gigabytes of memcpy. So the
  cell is *always* rasterized once into a `/BBox`-sized buffer and composited
  at each lattice position, with the position rounded to whole device pixels.
  A rotated or non-integral lattice therefore carries up to half a pixel of
  placement error per cell, which is the price of the only strategy that runs.
  Two strategies would also have meant two sets of rounding, and a file that
  crossed the threshold would have changed appearance for no reason a reader
  could see.
- **Over budget, nothing is painted.** The plan degrades to the cell's average
  colour. That is a plausible picture of something the engine did not draw: a
  flat wash where the file asked for a hatch reads as content, and nobody
  reports it. An unpainted area reads as the gap it is (ruling 2), and the
  warning names the pattern.
- **The recursion budget is not shared with the form cap.** The plan says
  "one recursion level" here and, under Text, that the budget is shared so
  that "Type 3-in-pattern-in-Type 3 bombs hit one ceiling". A cell is run by a
  *fresh* interpretation, so the interpreter's form-depth counter starts at
  zero again and cannot see a pattern nesting inside a pattern at all. The
  renderer threads its own depth through the tile request, and the number is
  smaller than the form cap because pattern nesting multiplies by the lattice
  count at every level where form nesting adds.

Uncolored patterns (paint type 2) render the cell as coverage only and take
the color of the slot whose `SCN` named the pattern — the *stroking* slot on a
stroke, which is not the same thing as "the current fill color".

### Text

Upright glyphs arrive as cached coverage masks from 05 wave 2 and composite
at their device positions; rotated/skewed/stroked text takes the outline
path directly through 07. Modes 0–2 fill and/or stroke, 3 paints nothing,
and 4–7 additionally union glyph coverage into a pending text-clip mask that
installs at `ET` and lives until the matching `Q`. Type 3 glyphs re-enter
the interpreter with the char proc, `FontMatrix` composed into the text
matrix; after `d1`, color operators in the proc are ignored and the glyph
paints in the current fill color, per 9.6.5. The recursion budget is shared
with the 05/06 budget so Type 3-in-pattern-in-Type 3 bombs hit one ceiling.

### Transparency

Staged as its own milestone because it is the second-hardest thing in this
phase and nothing else should wait for it. The model: buffers are
premultiplied RGBA in render space; `/ca`/`/CA` multiply into source alpha;
non-`Normal` blends unpremultiply the backdrop pixel locally and apply the
11.3.5 formulas per channel (separable set first; the non-separable HSL
four follow, needing the luminosity/saturation machinery of 11.3.5.3).
Transparency groups are not a separate compositing tree: a group XObject
pushes a bbox-sized buffer onto the device's buffer stack, mirroring PDF's
own nesting — isolated groups start transparent, non-isolated groups copy
the backdrop in and remove it again at composite time per the 11.4 group
formulas, knockout composites each element against the group's initial
backdrop. Soft masks from ExtGState `/SMask` render their group against a
`/BC`-filled backdrop, take luminosity as a fixed integer luma, and apply
the mask's `/TR` (which is honored — unlike the gstate transfer function,
see the table). A page-level `/Group` renders the page as an isolated group
so blend modes meet the page backdrop correctly. The honest corner:
non-separable blends inside non-isolated knockout groups are where the
oracle renderers disagree with each other; we implement the spec formulas,
pin our own fixtures, and let the perceptual budget absorb the rest.

*Amended, August 2026, on building it — gap
[11](gaps/11-transparency-groups.md).* Three corrections, and the second is
the substantive one.

**Buffers are straight-alpha, not premultiplied.** The paragraph above says
premultiplied RGBA; the `Canvas` has never been that. `clear` and `encode`
write straight colour and `pixel` reads it back, and the compositing formula
was corrected to 11.3.6's straight-alpha source-over before this gap started.
Nothing here depends on the choice — the blend formulas unpremultiply locally
either way — but the sentence was describing a buffer that does not exist.

**A non-isolated group's buffer carries two alphas, not one.** "Non-isolated
groups copy the backdrop in and remove it again at composite time" is right
and incomplete: 11.4.7.2's removal is `C = Cn + (Cn - C0)·(a0/agn - a0)`, and
`agn` is the group's *own* accumulated alpha, which is not the alpha the
buffer holds once the backdrop has been composited in. With an opaque
backdrop — every page — the two are related by a division that has already
lost `agn`. So the compositing function takes the initial backdrop's alpha as
a separate input: colour renormalises against the union, the alpha channel
accumulates the group's own. Any implementation that keeps one alpha per
pixel silently omits backdrop removal, and the result is a page that is
*slightly more muted*, everywhere there is a group.

**The group buffer is bounded by the clip, not by `/BBox`.** 8.10.2 has
already installed the `/BBox` clip by the time the group opens, so the clip
is the box intersected with everything enclosing it — never larger, usually
smaller, and free. A soft mask's buffer is the exception and is bounded by
its own group's `/BBox` alone, because the mask applies to paints that have
not happened yet and may be anywhere.

Two items in the paragraph above remain unbuilt and are not in gap 11's
scope: the **page group**, and blending in a group's own `/CS`. `/AIS` is
still recorded and ignored, and knockout's shape/alpha split is the one place
that would notice — the buffers carry one number, so a knockout restore is
exact at full coverage and at none and weights between them on an
anti-aliased edge.

### ExtGState coverage (8.4.5, Table 58)

| Entries | Policy |
| --- | --- |
| `LW LC LJ ML D` | Honored — forwarded to the 07 stroker |
| `Font` | Honored — bound through 05 |
| `CA ca BM SMask` | Honored — transparency machinery above |
| `AIS` | Recorded; treated as false, warning when true (shape/alpha split only observably differs in knockout/luminosity corners) |
| `RI FL SM SA` | Silently ignored — subsumed by AA and our flattening, no appearance change worth a warning |
| `OP op OPM BG BG2 UCR UCR2 TR TR2 HT` | Ignored with warning when the value is non-default — prepress simulation is out of scope, but the caller learns the page asked for it (ruling 10) |

### Optional content and annotations

At bind time, [04-document-semantics](04-document-semantics.md) hands over
the catalog's `/OCProperties` `/D` configuration; the device computes the
visibility set (`/OFF`, `/BaseState`, OCMD `/P` policies and `/VE`
expressions) once per document. The interpreter reports `/OC` marked-content
scopes and `/OC` entries on XObjects; the device suppresses paints inside
hidden scopes.

*Amended, August 2026, on building it — gap
[06](gaps/06-optional-content.md).* Two corrections, and the second is the
substantive one.

**The binder computes the visibility set, not the device.** Resolving an
`/OC` entry means walking indirect references — an OCMD's `/OCGs`, a `/VE`
expression's operands — and `tinker-pdf-render` sees no COS objects at all.
The set is computed in `tinker-pdf/src/optional.rs` alongside the rest of the
resource binding, and the device is handed a resolved `Layer { visible,
label }` per scope. This is the same division the colour and function binders
already use, and it is what keeps the render crate free of the object model.

**`/OC` on an XObject is reported as a marked-content scope, not as a
separate signal.** `Do` brackets the invocation in a hidden scope, so there is
one suppression mechanism rather than two. That is not only tidier: it is what
makes a hidden *image* skip its decode — a JBIG2 or JPX image in an off layer
is not a missing codec and must not get the ruling-2 placeholder, which would
be ink in a layer that is off — while a hidden *form* is still interpreted, so
text extraction and rendering keep agreeing about what the page contains
(ruling 7). Suppression is at the paint throughout: hidden content still
advances the text pen, still balances `q`/`Q`, and still installs a clip,
which 8.5.4 makes graphics state rather than a painting operation.

Annotations render only when `RenderOptions.annotations` is
true — the direct analogue of Tinker's `include_annotations`: each visible
annotation (Hidden/NoView flags honored, Popups skipped) renders its `/AP`
`/N` stream, `/AS`-selected when the appearance is a state dictionary, as a
form XObject through the 12.5.5 BBox→Rect mapping algorithm.

### The render contract

```rust
pub struct RenderOptions {
    pub transform: Matrix,          // composed on top of /Rotate + y-flip,
                                    // which the facade applies first
    pub clip: Option<IntRect>,      // device-space tile (ruling 5)
    pub format: PixelFormat,        // Rgb8 | Rgba8 | Gray8
    pub annotations: bool,
    pub background: Option<Color>,  // default opaque white; None needs Rgba8
    pub cancel: Option<CancelToken>,
    // non_exhaustive; Default + builders: scale(f32), at_dpi(f32)
}

pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub stride: usize,              // == width * bpp in v1, no padding
    pub data: Vec<u8>,
    pub warnings: Vec<Warning>,
}
```

Dimensions are `ceil` per axis of the transformed page box — the
[00-architecture](00-architecture.md) outward-rounding guarantee, pinned by
the A4 @ 150 dpi = 1240×1755 test. A `clip` yields exactly the clip's
dimensions. `stride` is contractually unpadded in v1 (Tinker's tests assume
`len == w*h*bpp`); the field exists so the contract can relax behind
accessors later without an API break. Bad options are hard errors with
distinct `ErrorKind`s so Tinker's `PAGE_OUT_OF_RANGE` / `ENGINE_ERROR` codes
map cleanly: non-finite or non-positive scale, singular transform, empty
clip, `background: None` without an alpha format.

Cancellation is cooperative, three cadences: per scanline band of the
target, every 256 device calls, and at strip boundaries inside long image
decodes. Cancelled renders return `Error::Cancelled` and no bitmap. A test
trips the token mid-decode and asserts acknowledgment within one band.

Degradation is the ruling-2 contract end to end: content problems become
placeholders plus provenance-carrying warnings on the `Bitmap`; the only
hard failures a well-formed call can see are cancellation and the option
errors above. `pdfcmp` budgets, the oracle-diff harness and the corpus
ratchet are specified in [14-testing-and-corpora](14-testing-and-corpora.md);
oracles are CI subprocesses, never dependencies (ruling 9).

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | `tinker-pdf-color` foundations: device spaces, Indexed, Cal/Lab approximations, ICC fallback policy; function Types 0/2/3 | Committed conversion/eval vectors pass; cross-target hash equality in the determinism job; fuzz targets clean; wasm build green | M |
| 2 | Type 4 calculator; Separation/DeviceN tint pipeline; LUT quantization | Calculator vectors incl. degree-trig and error-clamp cases; LUT worst-case error pinned under `pdfcmp` materiality; `type4` fuzz target clean | S |
| 3 | Device skeleton: fills/strokes via 07, gstate/clip stacks, `RenderOptions`/`Bitmap` with rounding, tiles, cancellation, degrade plumbing | Geometry/options ports of `render_pages.rs` green on vector fixtures: 595×842 @ 1.0, 1190×1684 @ 2.0, 1240×1755 @ 150 dpi, `Gray8` one component, tile rows byte-equal to full-page subregion, repeat renders identical, invalid scale/clip rejected | M |
| 4 | Text: modes 0–7 with clip accumulation, Type 3 recursion, glyph mask compositing | Text corpus within `pdfcmp` budget vs MuPDF oracle at 72/150/300 dpi; text-clip and Type 3 fixtures pass; shared recursion budget tested at the cap | M |
| 5 | Images: XObjects + inline, `/Decode`, SMask, color-key, ImageMask, strip cancellation, decoded-image LRU | Image corpus within budget; JBIG2/JPX fixtures degrade with placeholder + typed warning; mid-decode cancel acknowledged within one strip; cache bounded under adversarial reuse | L |
| 6 | Shadings 1–3, `sh`, shading patterns; mesh capability flag | Axial/radial fixture matrix (extend on/off, degenerate cones, `r0 = r1`) within budget; mesh fixtures degrade with warning; LUT path and direct eval agree on vectors | M |
| 7 | Tiling patterns: colored/uncolored, anchoring, step gaps/overlaps, cell budget | Pattern fixtures within budget incl. the anchoring fixture; XStep ≠ BBox width cases correct; adversarial tiny-step file completes under the budget with average-color fallback | S |
| 8 | Transparency: `ca`/`CA`, separable blends, groups, isolation/knockout, soft masks, page group; then the non-separable set | Per-blend-mode fixture chart within budget vs oracle; isolated/non-isolated/knockout unit fixtures match spec-derived expected images; luminosity soft-mask fixtures pass. **Done except the page group**, gap [11](gaps/11-transparency-groups.md); the oracle chart waits on gap [23](gaps/23-corpus-runner.md), and our own fixtures are the authority in the corners per the risk row below | L |
| 9 | Annotations `/AP` + `/AS` toggle; OC default visibility | Annotation fixtures differ exactly at annotation rects between toggle states; OCG/OCMD fixtures match oracle visibility incl. `/VE`; hidden-flag handling tested | S |
| 10 | Parity gate: full `render_pages.rs` port exact; corpus ratchet wired | The seven `render_pages.rs` behaviours pass verbatim (ruling 12); ≥ 95 % of corpus pages under `pdfcmp` budget vs the MuPDF oracle, pass-rate ratchet in CI can only rise; Tinker's `visual_regression.rs` port pixel-locked against our own goldens after one-time review | S |

M1–M2 are leaf-crate work with no dependency on either lane and should start
early, in parallel with semantics. At the low end of their ranges the
milestones sum to the top of the XL band; M5 and M8 carry the contingency,
and that is where it will be spent — images and transparency are where every
renderer's estimate goes to die.

## Dependencies

- **Needs:** [06-content-and-text](06-content-and-text.md) (interpreter and
  the `Device` trait), [07-rasterizer](07-rasterizer.md) (filler, stroker,
  spans), [05-fonts](05-fonts.md) wave 2 (glyph masks and outlines),
  [02-filters](02-filters.md) wave 2 (image codecs),
  [04-document-semantics](04-document-semantics.md) (page tree, resources,
  `/OCProperties`, annotation lists), and transitively
  [01-cos-and-object-model](01-cos-and-object-model.md) and
  [03-encryption](03-encryption.md). `tinker-pdf-color` itself needs nothing
  and starts day one.
- **Unblocks:** Checkpoint B and therefore
  [15-tinker-integration](15-tinker-integration.md); makes `pdfcmp` and the
  oracle-diff dashboards of [14-testing-and-corpora](14-testing-and-corpora.md)
  meaningful; [10-editing](10-editing.md) redaction preview and
  [12-creation](12-creation.md) appearance rendering reuse the device as-is.

## Risks

| Risk | Mitigation |
| --- | --- |
| Unhinted small text visibly worse than the hinted oracle — the project's declared top visual risk, surfacing here even though the cause lives in 05 | Stem darkening and gamma-aware coverage from 05/07; per-dpi perceptual thresholds; one-time human review then pixel-lock against our own goldens; Tinker's goldens regenerated at integration — "as good", never "identical to MuPDF" |
| Transparency group compositing (non-isolated backdrop removal, knockout) subtly wrong in ways page-level diffs blur away | Spec-formula unit fixtures with hand-derived expected images per case, not only oracle diffs; oracles disagree with each other in the corners, so our own fixtures are the authority there |
| Naive per-pixel function/color evaluation makes renders minutes long | LUT quantization with pinned error bounds; fused decode+convert passes; criterion benchmarks from M3 with a budget against `mutool draw` timings on the benchmark corpus, profiled before optimized |
| Memory blowups: group buffers, clip masks, pattern cells, decoded images on adversarial files | Everything bbox-sized and byte-budgeted; cell-count and allocation budgets with degrade-not-die fallbacks; fuzz targets run with allocation caps (ruling 1) |
| ICC-less color shifts eat the corpus-wide perceptual budget and mask real regressions | Per-fixture budgets rather than one global number; oracle-diff records shift statistics separately so a codec regression moves a different needle than a color-policy shift |
| Mesh-shading hit rate turns out high and the placeholder is ugly at scale | Ruling 3 gate schedules the implementation on evidence; placeholder honors `/Background` where present, halving the visual damage in the common styled-report case |
| Cancellation checks throttle throughput, or too-coarse cadence makes cancel laggy | Relaxed atomic loads at band/op/strip granularity only; a latency test pins acknowledgment within one band so cadence regressions fail CI |
| `pdfcmp` budget mis-tuned: too loose hides regressions, too tight blocks honest divergence | Per-page scores recorded, not just pass/fail; the CI ratchet fails on any worsening rather than on absolutes; thresholds tightened deliberately, as commits |
| Blend-mode math on premultiplied buffers accumulates rounding error | Blends computed on locally unpremultiplied values with committed per-mode vectors; per-mode fixture chart diffed against the oracle at M8 |

---

See [PLAN.md](../PLAN.md) for the phase map and checkpoints, and
[99-consistency](99-consistency.md) for the rulings cited throughout — they
win where this document and they disagree.
