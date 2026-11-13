# The bitmap toolbox

A rendered page is a `Bitmap`, and a caller does things to one after the
render: corrects its gamma, inverts it for a dark reading mode, tints it,
turns it grey, asks whether it is one colour or black and white, scales it,
takes every other sample, re-expresses it in another pixel format, or asks
the renderer for a page in forced colours for high contrast. Each of those is
a choice an implementer would otherwise make alone — which weights grey is,
where a gamma table rounds, what an inverted ink is, whether a scale may take
the sampler's pyramid — so this document makes them once, as closed forms a
test computes byte for byte, and places every one of them in
`tinker-pdf-raster`, which `cargo xtask libm` binds (ruling 4).

## Scope

- **The per-pixel maps**: gamma, invert, invert by luminance, tint and
  grayscale, each a function of one pixel, applied in place.
- **The reductions**: the colour count, the unicolour test and the monochrome
  test, each an answer about the whole bitmap.
- **Scale and subsample**: a bitmap resized to any width and height, and
  sampled by whole factors.
- **Conversion** between the eight pixel formats a `Bitmap` can hold
  (`Gray8`, `GrayA8`, `Rgb8`, `Rgba8`, `CmykA8`, `LabA8`, `Bgr8`, `Bgra8`),
  every ordered pair of them.
- **Forced colours**, with and without fill-to-stroke: a render option under
  which a page is drawn in two colours by the coverage of what it paints.
- What every one of them does with **alpha**, with **premultiplied** colour,
  with **ink** (`CmykA8`) and with **Lab** (`LabA8`), and with a bitmap's
  **stride**.

## Non-goals

- **RC-12's forced black** text or vectors. It changes the same paint colour
  forced colours change, and is that row's; whichever of RC-09.5 and RC-12
  lands second takes the other's case.
- **RC-17's colour-or-grey detection**, which asks whether a page needs colour
  at all. The monochrome test here asks something narrower — black and white
  only — and RC-17 is the row that says what "grey enough" is.
- **Colour management.** Every map is defined on the sRGB the canvas already
  decodes each format to; an ICC destination is RC-06.5's.
- **Sixteen-bit samples.** Every format here is a byte a channel (H-140 keeps
  the 8-bit arithmetic the top byte of any wider one).
- **Filters past the sampler's.** Scale is the rasterizer's own image sampler,
  never a second resampler with kernels of its own.
- **A map applied to a document.** The toolbox works on pixels; a recoloured
  document is ED-06's.

## Design

### Where the arithmetic is, and the door

All of it is in `crates/tinker-pdf-raster/src/toolbox.rs`, which is on
`PIXEL_PATHS` (`xtask/src/main.rs`), so no transcendental from the platform's
libm reaches a pixel: gamma's table is built through `tinker_pdf_math::pow`,
and every other form is integer arithmetic. The facade is not on that list,
which is why the arithmetic is not there either: each `Bitmap` method is one
call into the toolbox with the bitmap's own fields — its format, width,
height, stride, bytes and `premultiplied` flag — and adds nothing (ruling 11
puts the surface on the facade; ruling 4 puts the numbers below it).

The facade's methods, on `tinker_pdf::Bitmap`:

| Method | What it is |
| --- | --- |
| `gamma(&mut self, gamma: f64)` | every tone component through gamma's table |
| `invert(&mut self)` | every colour byte complemented |
| `invert_luminance(&mut self)` | lightness inverted, hue kept |
| `tint(&mut self, dark: Color, light: Color)` | every pixel's lightness on a ramp from `dark` to `light` |
| `grayscale(&mut self)` | every pixel its own lightness, in its own format |
| `colour_count(&self, limit: usize) -> Option<usize>` | distinct pixels, counted to `limit` |
| `is_unicolour(&self) -> Option<bool>` | every pixel the same |
| `is_monochrome(&self) -> Option<bool>` | every pixel opaque black or opaque white |
| `scaled(&self, width: u32, height: u32) -> Option<Bitmap>` | RC-09.3 |
| `subsampled(&self, x: u32, y: u32) -> Option<Bitmap>` | RC-09.3 |
| `converted(&self, format: PixelFormat, premultiplied: bool) -> Option<Bitmap>` | RC-09.4 |

`Color` is the canvas's straight 8-bit sRGB colour with alpha, re-exported by
the facade as `tinker_pdf::Color`.

### The pixels a map touches, and a bitmap that is not a picture

A bitmap is **a picture** when its width and height are not zero, its stride
holds a row (`stride >= width * components`), and its bytes hold every row
the other fields promise (`(height - 1) * stride + width * components`). That
is the condition `Bitmap::to_png` answers `None` for, and it is the one
condition here: a map on a bitmap that is not a picture changes nothing, and a
reduction answers `None`. The fields are public, so nothing may be assumed of
them (ruling 1).

On a picture, each operation walks the rows over `stride` and reads and writes
the `width * components` bytes of each and **nothing else**: a padded row's
padding is neither read nor written, so a padding byte can change no answer
and no map can change one.

### Tone, and what each format's tone is

The maps that are a curve over value — gamma, and the inversions — act on
**tone**, the quantity of light a component stands for:

| Format | Its tone components | Read as |
| --- | --- | --- |
| `Gray8`, `GrayA8` | the grey | the byte |
| `Rgb8`, `Rgba8` (and `Bgr8`, `Bgra8`) | red, green, blue | the byte |
| `CmykA8` | cyan, magenta, yellow, black | `255 - byte`: an ink is the light it takes away |
| `LabA8` | `L*` | the first byte; `a*` and `b*` are not tone |

The maps that need a whole colour — invert by luminance, tint, grayscale — and
the monochrome test decode a pixel to **sRGB** exactly as the canvas does
(`Canvas::pixel`): a grey replicated, RGB as it is, ink through 8.6.4.4's
relation (`canvas::cmyk_to_rgb`), Lab through `tinker-pdf-color`
(`canvas::lab_to_rgb`); and they encode a colour back exactly as the canvas
encodes a fill (`Canvas::encode`): grey by `Color::luma`, ink by the
maximum-undercolour-removal inverse (`canvas::rgb_to_cmyk`), Lab by
`canvas::lab_bytes`. **Lightness** is `Y = Color::luma` of that sRGB: the
Rec.601 weights in thousandths, `(299 r + 587 g + 114 b + 500) / 1000`, whose
weights sum to 1000 so a grey's lightness is that grey.

### Alpha and premultiplied colour

**Alpha is never mapped.** Every map writes the colour channels and leaves the
alpha byte as it was; tint ignores the alpha of the two colours it is given.

**Every map is defined on straight colour.** A premultiplied bitmap's pixel is
straightened first — `s = round(c * 255 / a)`, clamped to 255, and 0 where
`a` is 0, `Bitmap::straightened`'s arithmetic — mapped, and premultiplied
again — `round(s * a / 255)`, `Bitmap::premultiply`'s — so a premultiplied
bitmap stays premultiplied, a pixel of zero alpha stays all zero, and an
opaque pixel is exactly its straight form's. Below opaque the round trip costs
what premultiplying already cost: a channel is known to `255 / a` levels.

**The reductions read the bytes as they are stored**, premultiplied or not,
alpha included: two pixels are one colour when every byte of them is equal.

### The maps

Each is stated as its test computes it, for a tone or colour byte `v`:

- **Gamma `g`**: `T[v] = floor(255 * pow(v / 255, g) + 0.5)`, the table of
  256 built once a call through `tinker_pdf_math::pow`. Each tone component
  becomes `T[v]`, an ink `255 - T[255 - v]`, Lab's `L*` byte `T[v]` with `a*`
  and `b*` kept. `g = 1` is the identity; under 1 lightens and over 1
  darkens. A `g` that is not finite and greater than zero changes nothing.
- **Invert**: every colour byte `v` becomes `255 - v` in every format. For
  light that is the complement; for ink it is the ink of the complementary
  light, since the complement of `255 - v` is `v`; for Lab it is `L*` turned
  over and `a*`, `b*` reflected about 127.5, the opponent colour within the
  encoding's rounding.
- **Invert by luminance**: lightness inverted and chroma kept. In sRGB each of
  `r`, `g`, `b` becomes `clamp(c + 255 - 2Y, 0, 255)`, which is T.871's
  YCbCr with `Y` replaced by `255 - Y` and `Cb`, `Cr` kept, clamped where the
  result leaves the cube. A grey becomes `255 - v`. Lab's `L*` byte becomes
  `255 - L` with `a*` and `b*` kept, which is the same statement in a space
  where lightness is a coordinate. Ink is decoded to sRGB, mapped and encoded
  back, so its split is re-derived: a rich black comes back as `K` alone, as
  every ink conversion through light does.
- **Tint `(dark, light)`**: each pixel's lightness `Y` places it on the ramp
  from `dark` to `light`; each sRGB channel is
  `(dark * (255 - Y) + light * Y + 127) / 255` in integers, and the ramp's
  colour is encoded into the bitmap's format. Black goes to `dark`, white to
  `light`, and a grey between.
- **Grayscale**: each pixel becomes its own lightness. A grey is unchanged;
  RGB becomes `(Y, Y, Y)`; ink becomes `rgb_to_cmyk(Y, Y, Y)`, which is
  `(0, 0, 0, 255 - Y)`; Lab keeps its `L*` and sets `a*` and `b*` to the
  encoded zero, 128 — the neutral of the same lightness, which is Lab's own
  definition of grey.

### The reductions

- **The colour count, to `limit`**: the number of distinct pixels, every byte
  compared, or `limit` once that many are found; counting stops there, so
  what it holds is at most `limit` pixels whatever the bitmap's size.
  `limit = 0` answers 0.
- **The unicolour test**: every pixel equal to the first, every byte compared.
- **The monochrome test**: every pixel opaque black or opaque white — its sRGB
  is `(0, 0, 0)` or `(255, 255, 255)` and its alpha, where the format has one,
  is 255. An ink pixel of all four inks at 255 is black; a premultiplied
  bitmap's opaque pixels are its straight ones.

### Scale and subsample (RC-09.3)

**One resampler.** `scaled(width, height)` samples the bitmap through the
image sampler's area filter — `area_taps`, the sum `draw_image` takes every
area-filtered pixel through — over a grid of the target size that the bitmap
covers exactly (`tinker_pdf_raster::image::resample`). Destination column `X`
of `width` covers `[X W / width, (X + 1) W / width)` of a source `W` samples
across, row `Y` likewise of `H` down, and every sample under that rectangle is
weighted by the area of it the rectangle covers, so each byte is

    (Σ ox(X, i) · oy(Y, j) · s(i, j) + W H / 2) / (W H)

in integers, `ox(X, i)` the overlap of `[X W, (X + 1) W)` with
`[i width, (i + 1) width)` — the span and sample `i` measured in
`1 / width` of a sample — and `oy` the same down in `1 / height`; the weights
of a pixel sum to `W H`, and the division rounds half up. That is one rule
down and up, the rule a draw without `/Interpolate` magnifies by: a pixel
inside one sample is that sample, and one across a boundary is the two
weighted by their share, so a scale up keeps the picture's edges where a
bilinear blend would soften every one.

*Why the spans are planned in integers rather than through `draw_image`.* A
draw maps its unit square through a matrix, finds a pixel's centre through the
matrix's floating-point inverse, and quantises the span to 1/65536 of a sample;
`1 / width` is not a binary fraction for most widths, so a centre lands an ulp
off the rational one, and at a whole ratio, where half of all 2:1 averages are
exact ties, the ulp decides which way the tie breaks. A scale has no matrix —
the grid is the bitmap's own, multiplied — so its spans are the rationals
themselves, measured exactly, and summed by the same filter.

The sampler takes three channels of colour and an alpha, and its weights are
linear in a sample, so a format of more channels than three is handed over as
three-channel groups — ink's four as two — over the same weights and gathered
back, every channel through the same footprint, and alpha rides the sampler's
own alpha. **The toolbox never takes the pyramid**: a one-off resize
integrates exactly at any ratio, past 128:1 included, whose cost is the
source's samples once a line they reach — the pyramid exists to bound the
*same* image drawn many times (`docs/design/image-edges.md`), which a resize
is not. A channel's sum is at most `255 W H`, held in 64 bits for any source
under 7 · 10^16 samples, which no buffer holds; past it the scale is refused.
Colour is averaged premultiplied, so a transparent sample's colour carries no
weight; a straight bitmap is premultiplied for the average, `round(c a / 255)`,
and straightened after, `round(c · 255 / a)`. A scale to the bitmap's own size
is its bytes, its stride and padding included; a size of zero is no picture,
`None`, and so is a result whose bytes cannot be allocated, which the toolbox
reserves fallibly because its size is the caller's to ask for.

`subsampled(x, y)` keeps one sample of each `x` by `y` block, the first: the
result is `ceil(width / x)` by `ceil(height / y)`, and its pixel `(X, Y)` is
the source's `(X * x, Y * y)`, every byte as stored. A factor of zero is no
picture, `None`; a factor of one on both axes is the bitmap.

Both hand back a bitmap whose rows are packed, in the source's format and
premultiplied state, carrying the source's warnings — the picture is still
the render they describe — and no `stats`, which counted a render this was
not.

### Conversion (RC-09.4)

`converted(format, premultiplied)` is `Canvas::extract`'s per-pixel relation:
each pixel decoded to straight sRGB and alpha as the canvas decodes it and
encoded into `format` as the canvas encodes a fill — grey by luma, ink by
8.6.4.4's relation and its maximum-undercolour-removal inverse, Lab by
`tinker-pdf-color` — alpha carried where both formats have it, 255 where the
source has none, and dropped, the straight colour kept, where the target has
none. A premultiplied source is straightened first; the result is
premultiplied when `premultiplied` asks and the target has alpha. `Bgr8` and
`Bgra8` are `Rgb8` and `Rgba8` with the colour bytes in the other order. A
format converted to itself, premultiplied as it already is, is the same bytes:
same-format copies never go through a `Color`, which is
`Canvas::source_from`'s rule, because an ink buffer's split does not survive
the trip.

### Forced colours, and how they reach the renderer (RC-09.5)

`RenderOptions::forced_colours: Option<ForcedColours>`, with
`ForcedColours { background: Color, foreground: Color, fill_to_stroke: bool }`.
A fill becomes a stroke only where the path is known, so this is the
renderer's and not a pass over a finished `Bitmap`.

**The page is drawn in its coverage, then tinted.** Under forced colours the
renderer draws the page black on white into `Gray8`, whatever space a page
group declares: every fill, stroke and glyph paints opaque black by its own
coverage — whatever colour, DeviceCMYK ink or pattern it was given, so a
shading or tiling pattern's fill is its shape, and a stencil mask, which
paints the fill colour, is black too; an image, `sh` and a mesh paint their
own colours, which a grey canvas holds as their lightness, `Y`, so their
darkness is coverage too; constant alpha, blend modes, soft masks, knockout
and a group's colour space are not applied, so every paint is opaque and
`Normal` and the coverage is what the page's marks make of white. The
renderer does it by handing each paint a copy of its state whose paint is
opaque black, `Normal`, with no ink and no pattern (`Renderer::painting`),
the pen, the transform and the text state left the page's.

The renderer's answers to the interpreter's three questions stay an ordinary
render's, because a display list recorded once replays into either: a group
is opened, composited at full strength in the page's grey with no knockout,
and a soft mask's group is accepted and drawn into nothing — its paints are
skipped, since nothing reads them — and is never installed. That buffer is
then tinted by the toolbox (`toolbox::tint_grey`), `dark` the foreground and
`light` the background, into the format the options ask for, opaque. So every
pixel is the background plus its coverage times the difference,
`(foreground * (255 - v) + background * v + 127) / 255` for the black-on-white
grey `v`, written as a canvas of the format is filled, **exactly**: the
rounding is the tint's alone, not one a composite of two colours took at every
overlap. The page starts at the background whatever `transparent` asks, so a
forced render is opaque; `premultiplied` then changes no byte of it. No
transfer function (10.5) applies to a forced render, though its paints keep
the graphics state's: the page's colours are what forced colours replace, and
the tint is the final colour. Black generation and undercolour removal have
nothing to act on, since nothing is turned into ink before the tint.

**Fill-to-stroke** replaces the interior of every path fill (`f`, `f*` and the
fill of `B`, `B*`, `b`, `b*`) by the stroke of the same path at width 0, solid,
under the same clip and with the caps, joins and miter limit in force — the
renderer's thinnest line (8.4.3.2), which a stroke of `0 w` draws too, at
eight tenths of a device pixel anti-aliased and a whole one hard-edged, at any
scale. Glyphs, images and shadings are drawn as forced colours draw them, and
a Type 3 glyph is a glyph: its procedure's fills arrive inside the text object
that shows it (9.6.5), and a fill inside a text object stays a fill. A page of
fills so drawn is byte for byte its twin that strokes the same paths with
`0 w [] 0 d`, both in forced colours.

Off by default, so no fingerprint and no golden moves. A tile is the same
pipeline over a smaller canvas (ruling 5), and the tint is a per-pixel map, so
a tile of a forced-colour page is the page's subregion byte for byte.

## Milestones

1. **RC-09.2, the per-pixel toolbox.** `crates/tinker-pdf/tests/bitmap_toolbox.rs`
   `each_map_is_its_closed_form_over_a_committed_render` and
   `each_count_and_test_is_its_closed_form_over_a_committed_render`: over every
   reviewed golden in `tests/goldens/`, read through `golden_support`'s PPM
   reader, in every one of the eight formats, and over each with an alpha plane
   the test lays on, straight and premultiplied, every byte and answer equals
   the closed form computed in the test — gamma at exponents of one half, one
   and two, whose powers the test computes exactly in integers, `round(sqrt(255
   v))`, `v` and `(v^2 + 127) / 255`, none of them within a rounding of `pow`
   of a tie; `a_padded_stride_is_neither_read_nor_written`.
2. **RC-09.3, scale and subsample.** `bitmap_toolbox.rs`
   `a_scaled_render_is_the_area_average_the_test_computes`, at whole and
   fractional ratios, down and up, and
   `a_subsampled_render_is_the_samples_the_test_takes`, over the goldens; a
   scale to the same size returns the same bytes.
3. **RC-09.4, conversion.** `bitmap_toolbox.rs`
   `every_conversion_is_the_relation_the_test_computes`: all 56 ordered pairs
   of the eight formats over the goldens, every byte computed in the test from
   the relation above; a format converted to itself is the same bytes.
4. **RC-09.5, forced colours.** `crates/tinker-pdf/tests/render_options.rs`
   `forced_colours_are_coverage_over_the_two_colours`: every pixel is the
   background plus coverage times the difference, coverage read from the
   black-on-white render of the same page, computed in the test;
   `fill_to_stroke_draws_each_fill_as_its_stroke`: a page of fills renders
   byte-identical to its twin stroking the same paths at width 0, solid;
   `render_regions.rs` holds a forced-colour tile to its page; both builds.
5. **RC-09.6, fingerprints.** `bitmap_toolbox.rs`
   `the_toolbox_is_stable_across_targets` and `render_options.rs`
   `forced_colour_output_is_pinned`, in the default and `bundled-fonts`
   builds on Linux, Windows and `wasm32-wasip1`, both entered in K-07's
   determinism roster (`xtask/src/determinism.rs`).
6. **RC-09.7a, speed rows.** In MS-04's render group, unmeasured: a 150-dpi
   render scaled to a half and to two-thirds, and the text page with an
   embedded face at 150 dpi in forced colours with fill-to-stroke beside its
   default render. The per-pixel maps are one pass over the bytes and get no
   row. RC-09.7b takes the figures in the timing phase.

## Risks

| Risk | Mitigation |
| --- | --- |
| Gamma's table is where a platform `pow` would show first, and a byte that differs by target is a determinism bug | The table is built through `tinker_pdf_math::pow` in a crate `cargo xtask libm` binds; RC-09.6 pins its output on every target, and a hash two targets disagree on is a bug, never a table edit |
| A form rounds one way in the code and another in its test | Every form above is stated in the integers or the one `floor(x + 0.5)` its test computes, and the tests compute them independently of the toolbox |
| A premultiplied map drifts at low alpha | Stated, not hidden: the round trip is the precision premultiplying already had, and an opaque pixel is exact |
| An ink map re-derives the split, so a rich black becomes `K` alone | Only where a map goes through light (invert by luminance, tint, grayscale), said in each; invert and gamma act on the ink itself, and a same-format conversion never goes through light |
| Lab's encoding is a choice, not the space | Each Lab rule is stated on the encoded bytes — `L*` as `L/100`, `a*`, `b*` as `+ 128` — and is exact on them |
| The colour count holds every distinct pixel of a large page | It holds at most `limit`, and stops there |
| A padded or short buffer is read past | A bitmap that is not a picture is refused whole, before a byte is touched, and padding is outside every row's slice |
| Forced colours composited in two colours round differently at every overlap than the coverage they stand for | They are drawn as coverage and tinted once, so the closed form holds by construction |
| The pyramid's per-level rounding breaks scale's area average past 128:1 | The toolbox never takes it; a one-off resize integrates exactly |
| A span found through a floating-point inverse breaks the average's ties one way or the other | A scale plans its spans as the rationals they are, in `1 / width` and `1 / height` of a sample, and sums them with the sampler's own filter |
| Fill-to-stroke reaches a fill whose path is not known | Only path fills become strokes; glyph, image and shading paints are not paths and are drawn as forced colours draw them, and a fill inside a text object is a Type 3 glyph's and stays a fill |
| A display list recorded for an ordinary render disagrees with a forced renderer about a group or a mask, and a replay drops the bracket's content | The forced renderer answers the interpreter's three questions as an ordinary one does — it opens every group and accepts every soft mask's group — and applies neither: a group composites at full strength, a mask is never installed |
