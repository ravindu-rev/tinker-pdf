# LCD sub-pixel text and text contrast

*For [ROADMAP](../ROADMAP.md) row RC-10.* A liquid-crystal panel lights each
pixel through three vertical stripes, red, green and blue, side by side. A
glyph's coverage measured a stripe at a time places its edges three times as
finely across the line as one measured a pixel at a time, and a panel shows
that placement. This document fixes how a glyph's coverage is measured a
stripe at a time and filtered into three coverages a pixel, how those three
are composited, where text keeps today's single coverage, the contrast
control that maps every glyph coverage through one curve, and how a tile
keeps the filter's reach in the page's frame. Everything is integer, so a
stripe is the same byte on every target (ruling 4).

## Scope

- **Stripe coverage** (RC-10.2): a path filled at three times the horizontal
  resolution by the same `fill`, over the region widened by the filter's
  reach in the page frame, then filtered into three coverages a pixel, in
  RGB or BGR order.
- **The filter**: its taps, its normalisation and its reach.
- **Compositing three coverages a pixel** (RC-10.3): each colour channel at
  its own coverage, the alpha channel's rule, the clip and the soft mask.
- **Where stripes give way to grey** (RC-10.4): the canvases, blend modes,
  groups, patterns and glyph kinds that keep one coverage a pixel.
- **The contrast control** (RC-10.5): its levels and its curve, as integers.
- **Tiles** (ruling 5): how a tile keeps the filter's reach in the page frame,
  and the glyph cache's key.
- **The fingerprints and the speed row** (RC-10.6, RC-10.7a).

## Non-goals

- **Vertical stripes and other layouts.** A panel turned a quarter turn has
  its stripes across the line rather than along it, and a panel whose
  sub-pixels sit in triangles or diamonds has no stripes at all; the orders
  are horizontal RGB and BGR.
- **Graphics in stripes.** A filled or stroked path, an image's edge and a
  mesh's silhouette keep one coverage a pixel: a panel's stripes sharpen the
  vertical stems of small text, which is the case worth three times the fill.
- **Sub-pixel placement of a glyph's origin.** A glyph's origin keeps the
  sixteen phases a pixel P-06 gives it on each axis; a stripe changes how
  finely an edge is measured, not where a glyph stands.
- **Gamma-correct mixing of the three channels.** Coverage composites
  linearly, as grey coverage does; the contrast control is the knob for how
  heavy text looks.
- **Stripes in an output that is not a raster.** `to_svg`, text extraction
  and every vector writer are untouched.
- **Stripes on a page that is a layer.** A transparent page
  (`RenderOptions::transparent`) has no backdrop for a stripe to be judged
  against; its text is grey (*Where stripes give way*, below).

## Design

### Stripe coverage

A stripe is a third of a pixel wide. Stripe `k` of a row is the device
interval `[k/3, (k+1)/3)` across, the whole row down, in the page's frame:
the stripes of pixel `p` are `3p`, `3p + 1` and `3p + 2`.

**The stripes are measured by `fill` itself.** The path is mapped by
`(x, y) → (3x, y)` — `Path::mapped` with `a = 3` and every other entry of the
identity — and filled under its own rule, at its own tolerance, by the same
`fill` every shape goes through, over stripe columns as though each were a
pixel. So a stripe's coverage is the grid's: sixteen sub-scanlines a row,
exact spans in 1/256 of a stripe, each crossing to the nearest unit, 4 096
units a stripe and its byte `min(255, units / 16)`. The map is exact in `y`
and rounds `3x` once, the same way wherever it is computed, and it is taken
about the page frame's own `x = 0`, which every canvas shares.

**The region.** A stripe mask is asked for over a region of pixels, `x0` to
`x0 + width` across. The filter reads two stripes either side of the one it
writes (*The filter*), so the fill covers stripe columns `3(x0 − 1)` to
`3(x0 + width + 1)`: the region widened by one whole pixel each side, in the
page frame. Rows are the region's own; the filter reads none above or below.

### The filter

Each stripe's coverage `c[k]` is filtered into `s[k]` by five integer taps,
`[1, 2, 3, 2, 1]`, and a rounded divide by their sum:

```text
s[k] = (c[k−2] + 2·c[k−1] + 3·c[k] + 2·c[k+1] + c[k+2] + 4) / 9
```

in integers, `u32` throughout; the largest sum is `9 × 255 + 4`, and
`(9 × 255 + 4) / 9` is 255, so a filtered stripe is a byte.

Why these taps:

- **They are a pixel's average taken twice.** `[1, 2, 3, 2, 1]` is
  `[1, 1, 1]` convolved with itself, and `[1, 1, 1] / 3` averages the three
  stripes a pixel wide around a stripe. Its response has a double zero at one
  cycle a pixel — the period of the stripes themselves — which is the
  frequency colour fringes sit at; one pass leaves a single zero there.
- **Unit gain at zero frequency.** The taps sum to the divisor, so stripes
  that are all `c` filter to `c`: a glyph's solid interior is 255 in every
  channel, and a **horizontal edge**, whose stripes in a row all hold the
  same partial coverage, filters to that coverage in all three channels and
  composites exactly as grey does.
- **Symmetric**, so filtering moves no edge.
- **Short.** Two stripes either way is less than one pixel's reach, so the
  fill is widened by one pixel each side and no more.
- **Integer**, with a rounded divide rather than a float sum (ruling 4).

### Order

A pixel's three coverages are kept in the canvas's channel order, red, green,
blue, so the canvas never reads an order:

| Order | Red | Green | Blue |
| --- | --- | --- | --- |
| `Rgb` | `s[3p]` | `s[3p + 1]` | `s[3p + 2]` |
| `Bgr` | `s[3p + 2]` | `s[3p + 1]` | `s[3p]` |

A `Bgr` mask is the `Rgb` mask with its red and blue coverages exchanged at
every pixel.

### The stripe mask

`StripeMask` (`crates/tinker-pdf-raster/src/stripes.rs`) is `Mask`'s
counterpart: `x0`, `y0`, `width`, `height` in device pixels and `data`, one
`[u8; 3]` a pixel — red, green and blue coverage — row-major, zero outside
itself. `fill_stripes` and `fill_stripes_with` take what `fill` and
`fill_with` take and the order, and return one; the scratch is the caller's
`FillScratch`, which the three-times fill sweeps through like any other.
`StripeMask::intersect_in_place` multiplies a `Mask` into all three channels
alike, each product rounded as `Mask::intersect_in_place` rounds one.

### Compositing three coverages a pixel

`Canvas::fill_stripes_inked` takes what `fill_mask_inked` takes, with a
`StripeMask` for the `Mask`, and composites on `Rgb8` and `Rgba8`:

- **Each colour channel is the single-coverage composite at its own
  coverage.** Red is the red byte `fill_mask_inked` would write over that
  pixel at the red coverage, under the same colour, alpha and blend mode;
  green and blue likewise. The general blend and the opaque path are the ones
  `fill_mask_inked` takes, with the channel's coverage in place of the
  pixel's: over an opaque pixel under `Normal` at an alpha of one, a channel
  is `min(mul255(cs, c) + mul255(cb, 255 − c), 255)` at its own `c`.
- **The alpha channel is the single-coverage composite's alpha at the mean
  of the three coverages**, `(r + g + b + 1) / 3` in integers. Over an opaque
  pixel that is 255 whatever the coverages are. **On a transparent page** a
  stripe pixel's alpha is the alpha a grey pixel of that mean coverage gets,
  so the pixel stands for as much of the shape as its grey twin does; its
  three colour channels are each their own composite over the transparent
  backdrop, which is as near to right as a pixel with no backdrop can be,
  and why the renderer keeps such a page's text grey.
- **A clip or a soft mask multiplies all three coverages alike**, before the
  composite, through `StripeMask::intersect_in_place`.
- **Equal coverages are grey.** A pixel whose three coverages are one `c` —
  every pixel of a solid interior and of a horizontal edge — composites to the
  bytes `fill_mask_inked` writes at `c`, alpha included. That is the guard
  that text in stripes moves no pixel grey text would draw alike.

On a format with no red, green and blue channels — `Gray8`, `GrayA8`,
`CmykA8` and `LabA8` — `fill_stripes_inked` composites the grey mask of each
pixel's mean coverage, as `fill_mask_inked` would; the renderer never asks
for that, and the canvas answers it rather than refusing.

### Where stripes give way to grey

`RenderOptions` gains a sub-pixel order: off, the default, `Rgb` or `Bgr`.
With it on, a glyph is drawn in stripes when **every** one of these holds, and
in grey exactly as today otherwise:

| Condition | Why |
| --- | --- |
| Text render mode 0, a fill and nothing else (9.3.6) | A stroked glyph's outline is a pen's, not a typeface's stem, and a clipping mode's glyphs become a clip, which has one coverage a pixel |
| A solid colour, not a pattern | A pattern's cell is drawn and sampled as an image, and sampling loses a stripe's place |
| Blend mode `Normal` | A non-separable mode (11.3.5.3) mixes the three channels, so no channel has a coverage of its own; a separable one would allow stripes, and keeps grey so that one rule holds for every mode but `Normal` |
| Not a Type 3 glyph | Its procedure paints graphics, which are grey |
| The page's own canvas, in `Rgb8` or in `Rgba8` without `transparent` | A light page with a backdrop under every pixel. An ink page (`CmykA8`) composites quantities of ink, not light; a grey page has no channels to stripe; a transparent page has no backdrop |
| Not inside a transparency group, a soft mask's group or a tiling pattern's cell | Each of those is a buffer composited later, over a backdrop it does not hold, or sampled as an image |
| Text at eight bits, with `antialias` on | A quantised or thresholded coverage has lost the fractions the filter distributes ([anti-aliasing](anti-aliasing.md)) |

A glyph's fill alpha, a clip and a soft mask are all allowed: each multiplies
the three coverages alike. Both routes take stripes — a glyph drawn from its
cached mask (P-06) and one drawn as a path because its mask would be too
large — and `Page::render`, `render_form`, `render_annotation` and
`DisplayList::render` all honour the option, since each draws through the
renderer.

### The contrast control

`RenderOptions` gains a text contrast level `L`, `0` to `8`, default `0`.
Every glyph coverage — grey, or each of a stripe pixel's three — goes through
one curve before the clip and the soft mask multiply into it:

```text
T_L(c) = c + (L·c·(255 − c) + 1020) / 2040        integer division
```

- `T_0` is the identity, so the default level moves no byte.
- `T_L(0) = 0` and `T_L(255) = 255`, so empty stays empty and whole stays
  whole, and a solid interior is untouched at every level.
- **It is non-decreasing in `c`** at every level: `c + L·c(255 − c)/2040` has
  slope `1 + L(255 − 2c)/2040`, at least `1 − 8 × 255/2040 = 0` at `L = 8`,
  and adding the floor of a function to `c` cannot make two neighbours swap,
  since the function moves by more than −1 from one `c` to the next.
- **What it does**: a half-covered pixel, `c = 128`, becomes 128, 144, 160,
  176 and 192 at levels 0, 2, 4, 6 and 8 — thin stems darken, as a
  gamma above one would darken them, without a power function. A curve that
  is a polynomial needs no transcendental, so nothing reaches
  `tinker-pdf-math` and `cargo xtask libm` has nothing to bind.

The table is built once a render, 256 bytes a level, and a test computes it
from the formula. Only coverage passes through it: a constant alpha, a soft
mask's levels and the colour stay the document's, the boundary
`RenderOptions::antialias` already draws.

### Tiles keep the filter's reach in the page frame

A pixel's three channels read the stripes of the pixel to its left and the
pixel to its right. A tile reads them from the page:

- **The stripe columns are the page's.** The three-times map is taken about
  the page frame's `x = 0`, and every mask the crate hands a canvas is in the
  page's device pixels (`Canvas::origin`), so stripe `k` is the same number in
  a tile, a group's buffer and the page.
- **The widening comes from the page frame, never from the tile.** The region
  a paint asks for is widened by one pixel each side as geometry: the widened
  stripes' coverage is a function of the path alone, since the clip and the
  soft mask multiply after the filter, pixel by pixel. So, unlike a mesh's
  fringe (`draw_mesh_over`), nothing is kept inside a whole region: a pixel on
  a tile's edge reads the same nine stripes the page reads for it.
- **A cached glyph's stripes stand on the page's lattice.** A glyph's mask is
  made in its own frame and placed by a whole pixel (P-06), which is three
  whole stripes, so its stripes are the page's stripes moved by a multiple of
  three, and the filter, a convolution, commutes with that move.
- **The glyph cache's key gains the order and the contrast level**, so a mask
  made in grey is never composited as stripes, nor one made at one contrast
  level at another.

### API

```rust
pub enum SubpixelOrder { Rgb, Bgr }

pub struct StripeMask {
    pub x0: i32,
    pub y0: i32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<[u8; 3]>,
}

pub fn fill_stripes(path, rule, x0, y0, width, height, tolerance, order, stop) -> StripeMask;
pub fn fill_stripes_with(scratch, path, rule, x0, y0, width, height, tolerance, order, stop) -> StripeMask;

impl Canvas {
    pub fn fill_stripes_inked(&mut self, mask: &StripeMask, color: Color, ink: Option<[u8; 4]>, alpha: f64, mode: BlendMode);
}

// tinker_pdf::RenderOptions
pub subpixel: Option<SubpixelOrder>,   // None by default
pub text_contrast: u8,                 // 0 to 8, 0 by default
```

The two render options are passed through `render_layer_with` to the
`Renderer` as `antialias` is.

### Anti-aliasing levels

RC-11's text level acts on the same glyph coverage, and its design
([anti-aliasing](anti-aliasing.md), *Stripes and contrast*) says how the two
combine: stripes need the text's eight bits, and contrast comes before the
level.

## Milestones

1. **RC-10.2, stripe coverage.** `stripes.rs`'s `SubpixelOrder`,
   `StripeMask`, `fill_stripes` and `fill_stripes_with`, exported from the
   crate root. Exit: `analytic_coverage.rs`
   `stripe_coverage_is_three_times_horizontal_coverage_filtered` —
   rectangles at every third of a pixel against the grid's closed form for
   each stripe, filtered in the test; a diagonal and a quadratic outline
   against `fill` of the path mapped by `(3x, y)`, filtered in the test with
   the taps above; both orders — and `a_stripe_mask_over_a_tile_is_the_page_s`,
   tiles whose edges fall within the filter's reach of the shape's edges, each
   tile's stripes the page's (ruling 5).
2. **RC-10.3, the composite.** `Canvas::fill_stripes_inked`. Exit: `canvas.rs`
   `a_stripe_composite_is_three_single_channel_composites` — each channel the
   `fill_mask` composite of that channel's coverage alone, computed in the
   test, at alpha one and one half and over a transparent backdrop, the alpha
   byte the composite at the mean — and `bgr_order_swaps_the_outer_channels`.
3. **RC-10.4, LCD text.** `RenderOptions::subpixel` through
   `render_layer_with` to the renderer, `show_glyph` on both routes, the
   conditions of *Where stripes give way* and the cache key. Exit:
   `lcd_text.rs` `stripe_coverage_equals_three_times_horizontal_coverage_filtered`
   — text in `render_support`'s synthetic face at 1x, 1.5x and 3x, RGB and BGR,
   each channel's coverage the glyph outlines the test places, filled by
   `fill` at three times the horizontal resolution and filtered in the test;
   off, every byte unchanged — and `render_regions.rs`
   `an_lcd_text_tile_is_the_page_under_it`, in both builds.
4. **RC-10.5, contrast.** `RenderOptions::text_contrast` and the table. Exit:
   `lcd_text.rs` `contrast_maps_each_glyph_coverage_through_its_curve` — at
   three levels each glyph pixel's coverage is `T_L` of its uncontrasted
   coverage, computed in the test, in grey and in both orders; fills, strokes
   and images move no byte; the default level moves none; both builds.
5. **RC-10.6, fingerprints.** Exit: `lcd_text.rs`
   `lcd_text_is_stable_across_targets` — a page of the synthetic face in RGB
   and BGR at contrast levels 0 and 4, hashed as `determinism.rs` hashes a
   render — pinned on Linux, Windows and `wasm32-wasip1`, and entered in K-07's
   roster (`xtask/src/determinism.rs`); one hash serves both builds, the face
   being the fixtures' own.
6. **RC-10.7a, the speed row.** MS-04's text page with an embedded face at
   150 dpi rendered with RGB stripes, beside the same page in grey, in the
   render group, unmeasured until RC-10.7b.

## Risks

| Risk | Mitigation |
| --- | --- |
| A stripe moves a pixel grey text draws | Equal coverages composite as grey, byte for byte (`a_stripe_composite_is_three_single_channel_composites`); with the order off nothing reaches the stripe path, and `lcd_text.rs` holds every byte unchanged |
| A tile's edge pixel reads stripes the page does not | The widening is geometry in the page frame; `a_stripe_mask_over_a_tile_is_the_page_s` and `an_lcd_text_tile_is_the_page_under_it` put tile edges within the filter's reach |
| A cached grey mask composited as stripes | The order and the contrast level join the glyph cache's key |
| Colour fringes where the backdrop is not one colour | Inherent to stripes, and why every buffer composited later keeps grey; the filter's double zero at the stripe period keeps them to the edges |
| A stripe glyph costs three times a grey one's fill and a filter pass | Recorded by RC-10.7a; a target for it is the owner's (H-77) |
| A float creeps into the filter or the curve | Both are integer formulas a test recomputes; `cargo xtask libm` binds the render crate |
