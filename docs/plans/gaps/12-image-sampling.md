# Images are sampled nearest-neighbour, in the wrong crate

Every image is drawn with one truncating tap per destination pixel. A
photograph scaled down to a thumbnail loses most of its pixels and gains
aliasing; a logo scaled up has hard stair-steps. `/Interpolate`, which the
file uses to ask for smoothing, is parsed and read by nothing. When this is
done, images sample the way plan 07 already specifies. (M)

## What is wrong

`Renderer::blit` in `crates/tinker-pdf-render/src/lib.rs`:

```rust
let sx = ((u * f64::from(image.width)) as u32).min(image.width - 1);
let sy = (((1.0 - v) * f64::from(image.height)) as u32).min(image.height - 1);
```

One tap, truncating cast, no filtering, no mip level.

It is also in the wrong crate. `tinker-pdf-raster` exports `blend`, `canvas`,
`fill`, `geom` and `stroke` — there is no image entry point at all, so there
is nowhere to put a filter. Plan 07 specifies one:

```rust
Rasterizer::draw_image(&mut self, s: &mut Surface, img: &ImageSource<'_>,
                       t: Transform, interpolate: bool, alpha: u8, clip: &ClipStack)
```

and gives the whole policy matrix in `07-rasterizer.md:229-258`. None of it
was built, and STATUS calls phase 07 "complete".

`/Interpolate` (`/I` inline) is in the abbreviation table and is never read.
`DecodedImage` has no field for it, and no transform-derived scale is
available where the sampling happens.

A second nearest-neighbour sampler sits in the soft-mask resampling path in
`resources.rs`, with its choice documented; that one is defensible and is not
in scope here.

## Scope

- Move image sampling into `tinker-pdf-raster` behind an entry point of plan
  07's shape.
- Plan 07's four-row policy: at or above 1× with `/Interpolate` → bilinear; at
  or above 1× without → nearest; downscale to 2:1 → bilinear; beyond 2:1 → box
  pyramid, then bilinear.
- Read `/Interpolate` and carry it on `DecodedImage`.
- Make the scale factor available at the sample site, since the policy branches
  on it.
- Return pyramid levels to the caller so phase 08 can cache them, as plan 07
  says.

## Non-goals

- **Higher-order filters.** Lanczos and Mitchell are better and are not what
  the plan specifies; matching the plan matters more than beating it.
- **The soft-mask resampler.** Separate path, documented choice, different
  trade-off.

## Design

Plan 07 has already made the decisions, including the one that looks wrong at
first glance: **nearest at or above 1× without `/Interpolate`**. Upscaling a
screenshot or a barcode with bilinear blurs edges the file wants sharp, and
`/Interpolate` is how a file asks for the other behaviour. The reasoning is at
`07-rasterizer.md:241-247`; follow it rather than re-deciding it.

The box pyramid beyond 2:1 is what stops a 4000-pixel-wide photograph in a
thumbnail from aliasing into moiré. Levels are built lazily and handed back so
the caller can keep them for the next draw of the same image.

**Determinism.** Bilinear weights must be fixed-point or plain `f64`
arithmetic — no transcendental calls — so ruling 4 holds. The image fixtures
join `tests/determinism.rs`.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Image entry point in the raster crate; `blit` moved behind it | Existing image tests pass unchanged with the sampler relocated | S |
| 2 | `/Interpolate` read and carried | A fixture with and without the key takes different branches, asserted on the pixels | S |
| 3 | Bilinear | An upscaled gradient has no stair-steps; a 2:1 downscale keeps its midtones | M |
| 4 | Box pyramid beyond 2:1, levels returned | A 16:1 downscale of a fine checkerboard is flat grey rather than moiré | M |
| 5 | Determinism | Image fixtures hash identically across targets | S |

## Dependencies

**Needs first:** nothing.

**Unblocks:** honest corpus render comparison — nearest-neighbour downscaling
is a large perceptual difference against any reference renderer, so the corpus
budget cannot mean much until this lands.

## Risks

| Risk | Mitigation |
| --- | --- |
| Every image golden moves | **Corrected, August 2026.** There are no golden images in this repository. [16-build-sequence.md](../16-build-sequence.md)'s "re-baseline once, after item 14" refers to Tinker's MuPDF goldens, in a different repository, and lives in phase 15. The only pixel baseline here is `crates/tinker-pdf/tests/determinism.rs`, whose rule is the opposite of batching: update the table **in the same commit that caused the change**, naming the fixture, the code path and the measured delta — and if the two targets disagree instead, do not update it at all, because the table is the only thing that would ever report a determinism bug. In the event nothing moved: none of the six fixtures drew an image, which is why this gap adds a seventh |
| Moving the sampler across a crate boundary drags PDF types into a leaf crate (ruling 8) | The entry point takes bytes, dimensions and a transform — `ImageSource` in plan 07's sketch is deliberately PDF-free |
| Pyramid levels are memory the caller may not expect | Returned rather than retained, so the caching decision stays with phase 08 where the lifetime is known |

---

## As built — August 2026

Five milestones, five commits, in the table's order. The defect was still live
at `fe59c8c`, on every count: `blit` took one truncating tap per destination
pixel with no filtering and no mip level; `tinker-pdf-raster` exported
`blend`, `canvas`, `fill`, `geom` and `stroke` and had no image entry point at
all; `/Interpolate` was in the abbreviation table at `resources.rs:779` and
read at no other line in the workspace, with no field on `DecodedImage`; and
nothing derived a scale from the transform anywhere — `blit` held the matrix
and never asked it a question.

### What the entry point looks like, and why it stayed PDF-free

`tinker_pdf_raster::image` takes an `ImageSource` — width, height, three bytes
of colour per sample, an optional byte of coverage — a `Transform`, and an
`ImageDraw` carrying alpha, blend mode, clip, tint and a stop predicate. No COS
type crosses the boundary and `xtask dag` is unchanged, because no new edge was
needed: `raster` already depended on nothing but `math`.

Two pieces of PDF meaning were inside the loop and left with their names rather
than with their vocabulary. 8.9.6.2's stencil became `tint`: "paint this colour
and take only coverage from the image", which is a graphics idea a leaf crate
may hold. The cancellation check became `stop`, a `&dyn Fn() -> bool` asked
once per destination row, which keeps the existing mechanism working without
`CancelToken` reaching a leaf.

`Transform` is new to the crate. Fills and strokes take paths already in device
space, so the rasteriser had never needed a matrix; an image cannot be
pre-transformed the same way, because the mapping is what decides which sample
a destination pixel reads. Its `invert` is `tinker-pdf-render`'s formula copied
unchanged, degeneracy test included, so the move did not shift a placement by a
bit.

### The policy, and the numbers behind the milestone criteria

Plan 07's four rows, followed rather than re-decided, including nearest at or
above 1:1 without `/Interpolate`. The scale comes from the transform's column
lengths, one per axis, in 16.16 fixed point.

Both of milestone 3's criteria are perceptual claims, so both were turned into
measurements:

| Claim | Measurement | Nearest | As built |
| --- | --- | --- | --- |
| An upscaled gradient has no stair-steps | Largest jump between neighbouring output pixels; 8 samples rising by 32, magnified 16x | 32 | **2** (32/16) |
| A 2:1 downscale keeps its midtones | Mean, and the range around it; a one-sample checkerboard, 16x16 into 8x8, true mean 127.5 | one phase of the board: mean 0 or 255 | **mean 128, min 128, max 128** |
| A 16:1 downscale is flat grey, not moiré | Mean and variance; a one-sample checkerboard, 256x256 into 16x16 | flat, one phase | **mean 128, variance 0** |

The third needed the pyramid, and the test says so directly: at 16:1 four taps
land eight samples apart, and eight is a whole number of periods of that board,
so bilinear *alone* also returns a flat page — of black or of white. Both
answers are "flat" and only one is grey, so the test asserts the bilinear-only
result as well, and cannot pass by the two agreeing.

### How the pyramid level count was made target-stable

The obvious spelling is `ceil(log2(ratio / 2))`. `log2` is a transcendental:
`cargo xtask libm` refuses one on a pixel path because no two platforms round
it alike, and a *count* that comes out one different is not a rounding
difference in the output — it is an image at half the resolution. This is the
class gap 13 found in the flattener, where a float termination test changed the
number of segments rather than their positions.

So the ratio is built from `sqrt`, multiplication and division only — all
correctly rounded, so bit-identical everywhere — cast once into 16.16 fixed
point, and every comparison after that is between two integers. The loop
doubles its threshold rather than shifting the ratio down, because shifting
truncates: a ratio a hair over 4 would look like exactly 2 after one halving
and stop one short. The loop also stops when the axis reaches one sample, and
is capped at sixteen halvings, which takes any image a decoder can produce down
to a single sample.

Indexing is `checked_mul` throughout rather than saturating. `usize` is 32 bits
on wasm32 and 64 on the other three targets, and that is the width assumption
this gap could have carried.

### Levels are returned, not retained

`draw_image` takes a `&mut Pyramid` the caller owns, builds what it needs into
it and leaves it there; the crate holds nothing between calls. `Renderer::blit`
passes a fresh one and drops it, deliberately: reusing levels needs an
*identity* for the image, and two 512x512 photographs are indistinguishable to
a function that sees only a shape. That identity belongs to the resource cache,
which is phase 08's, where an image's lifetime is known.

### The fingerprint, and the fixture that had to be built twice

None of the six existing fixtures drew an image, so nothing moved in any of the
five commits — and the report meant nothing, which is the point milestone 5
exists to fix. The `image` fixture is six placements of one 32x32 image: two
magnifications differing only in `/Interpolate`, a 1.6:1 downscale, a 4:1 and
an 8:1 for one and two pyramid levels, and a rotated magnification whose
inverse has all four coefficients. 5262 pixels of ink out of 19 200, floor
2600.

Its content was wrong first. The original was a linear ramp with a one-sample
checkerboard, and injecting a pyramid one level short moved **nothing at all**:
a linear ramp is reproduced exactly by box averaging and by bilinear taps alike
at any depth, and every balanced two-tap average of a checkerboard is its mean.
The blue channel is now a quadratic scramble, which has a different mean over
every window. Same injection, hash moves.

Both targets agree on the new hash — x86_64 Windows and `wasm32-wasip1` under
wasmtime 47.0.3, 64-bit against 32-bit. The other six are untouched.

### Defect injection

Each defect applied alone, the whole workspace re-run with `--no-fail-fast`:

| Injected | Assertions that caught it |
| --- | --- |
| Bilinear weights transposed (u/v) | 2 — the gradient step measurement, and the fingerprint |
| Pyramid one level short | 4 — three unit tests, and the fingerprint (**nothing**, before the fixture content was fixed) |
| Pyramid one level too many | 4 — the same four |
| `/Interpolate` read and then ignored at the sample site | 4 — both end-to-end tests, the render-layer branch test, and the fingerprint |
| Policy row 2 inverted | 7 — everything about magnification |

### Amendments, and one correction to this document

The risk table's first row is corrected in place: there are no golden images in
this repository, and nothing was batched or deferred.

This document's own claim that "STATUS calls phase 07 complete" was already out
of date when this gap ran — the August audit had changed that row to "see gaps
12, 15" some commits earlier. It now reads "see gap 15".

`docs/plans/07-rasterizer.md` is amended in place on three points this gap had
to establish. Its API sketch is `Rasterizer::draw_image(&mut self, s: &mut
Surface, ...)`; there is no `Rasterizer` type, no `Surface` and no `ClipStack`
in this crate, and the shipped entry point is a free function over a `Canvas`
with an optional `Mask`. Its `ImageSource` is described as "premultiplied on
ingest" and the crate is straight-alpha throughout, which `canvas.rs` documents
as its one convention. And it says rotated placements walk the inverse
transform "with a 16.16 fixed-point DDA": the inverse is applied per pixel in
`f64` — correctly rounded, so ruling 4 holds — and fixed point appears where
the plan did not put it, in the weights and in the policy's own branch.

### Not done

- **The soft-mask resampler in `resources.rs`** is still nearest-neighbour,
  which was a non-goal and remains a documented choice.
- **Nothing caches a pyramid.** That is phase 08's, and the seam is the
  `&mut Pyramid` parameter.
- **The four-target claim is still two targets.** Linux and macOS come only
  from the CI matrix, as gap 25 records.
