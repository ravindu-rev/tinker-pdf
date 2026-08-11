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
| Every image golden moves | Land it with the other pixel-moving work and re-baseline once, per the ordering rule in [16-build-sequence.md](../16-build-sequence.md) |
| Moving the sampler across a crate boundary drags PDF types into a leaf crate (ruling 8) | The entry point takes bytes, dimensions and a transform — `ImageSource` in plan 07's sketch is deliberately PDF-free |
| Pyramid levels are memory the caller may not expect | Returned rather than retained, so the caching decision stays with phase 08 where the lifetime is known |
