# Sixteen bits a component

*For [ROADMAP](../ROADMAP.md) row RC-07.* Every buffer the renderer composites
in holds one byte a component today, and every stage between a content stream
and a `Bitmap` rounds to one: a colour resolved from its components, a pixel's
coverage, a blend mode's answer, a composite, a soft mask, an image sample.
This design carries two bytes a component through the same stages, so a page
comes back at 16 bits, and states how each 16-bit sample relates to the byte
today's render puts in the same place. The owner's answer to H-140 decides
that relation (ruling 23): **the top byte is today's 8-bit arithmetic, bit for
bit, the low byte refines it, and no 8-bit fingerprint or golden moves.**

## Scope

- **The 16-bit formats**: a counterpart for every `PixelFormat` the
  rasterizer has, or the reason for none, and the one byte order all of them
  store.
- **The relation** a 16-bit sample keeps with today's byte, and the two
  candidates RC-07.2 prices.
- **The arithmetic at 16 bits**, stage by stage: coverage; the compositing
  core and its two backdrop branches; the sixteen blend modes; groups, soft
  masks and knockout; images and image runs; meshes; shadings; resolved
  colour.
- **What a 16-bit `Bitmap` is in the facade**, and what `Bitmap::to_png`,
  `pdfcmp`, `tpdf` and the bindings do with one until the FO rows write one.
- RC-07.2 to RC-07.16's exit tests.

## Non-goals

- **16-bit files.** PNG at 16 bits is FO-05, TIFF FO-03 and PSD FO-04; until
  then a 16-bit bitmap is written as below.
- **HDR transfer** — FI-07c, which builds on this pipeline.
- **A bilevel or halftoned page from a 16-bit render.** A 1-bit page screens
  the grey of the `Rgba8` render (`docs/design/output-pixel-models.md`);
  screening sixteen bits buys nothing a threshold can show.
- **16-bit formats in the C ABI and the bindings.** `TpdfPixelFormat` keeps
  its four formats; SU-12 projects the facade's when it projects the BGR
  orders and the destinations.
- **Reading a 16-bit PNG into a 16-bit `Bitmap`.** `Bitmap::from_png` narrows
  sixteen bits to eight as it does today; widening it is FO-05's, which
  writes the files it would read.
- **A second relation beside the owner's.** The other candidate below is
  priced by RC-07.2's census, never implemented.

## Design

### The relation: today's byte, and a byte below it

A 16-bit sample `S` is an integer in `0..=65 535`; its **top byte** is
`S >> 8` and its **low byte** `S & 255`. Two relations to today's render were
open, and H-140 put them to the owner:

- **(A) Today's arithmetic, refined in a low byte.** At every place today's
  arithmetic rounds a value to a byte `v`, the 16-bit arithmetic computes the
  same formula at sixteen bits and keeps the answer inside `v`'s interval,
  `256·v ..= 256·v + 255`. The top byte of every 16-bit sample is therefore
  the byte the 8-bit render holds in the same place, bit for bit, and the low
  byte is the formula's own answer wherever it falls inside that interval and
  the interval's nearer end where it does not.
- **(B) A value carried at sixteen bits throughout.** Every stage computes at
  sixteen bits and rounds to sixteen bits, and nothing ties it to today's
  bytes. It is nearer the exact value, and its top byte departs from today's
  wherever the two roundings part: a component of 0.1981 is 51 at eight bits
  (`round(255·v)`, `byte` in `crates/tinker-pdf-color/src/lib.rs`) and
  12 982 at sixteen, whose top byte is 50.

**The owner chose (A)** (ruling 23, H-140), the relation RC-07's own exit
names, and **every later step implements (A)**. (B) is test code in RC-07.2's
census and nowhere else, so nothing here moves a render.

The definitions every stage below uses:

| Name | Value |
| --- | --- |
| `widen(b)` | `257·b`: a byte at sixteen bits, `b` in its top byte and again in its low one, and `b / 255 = widen(b) / 65 535` exactly |
| `hold(S, v)` | `S` clamped into `256·v ..= 256·v + 255`: (A) at one rounding point |
| `mul16(a, b)` | `(a·b + 32 767) / 65 535` in `u64`: `a·b / 65 535` rounded to nearest, which has no ties because 65 535 is odd and `2·a·b` even |
| `q16(x)` | `hold(round(65 535·x), q8(x))` for a value `x` in `0..=1`, where `q8(x)` is today's byte for it — `round(255·x)` where today rounds that way |

Two properties make `hold` the whole of (A). The ends are exact:
`q16(0) = 0` and `q16(1) = 65 535`, since `round(65 535·x)` is inside the
interval there. And the interval is never more than half a byte level from
the exact value: with `v = round(255·x)`, `65 535·x` lies within 128.5 of
`257·v`, and the interval reaches from `257·v − v` to `257·v + 255 − v`, so a
held value lies within half an 8-bit level of the exact one at worst and is
the 16-bit rounding of it wherever that falls inside. RC-07.2 counts how
often a value is held and how far.

**Twin arithmetic.** A stage working at sixteen bits computes today's 8-bit
answer from the top bytes of its 16-bit inputs — the same function, called,
never transcribed — and its 16-bit answer from the inputs themselves, and
stores `hold(answer16, answer8)`. Its inputs' top bytes are today's inputs at
that point, by the same rule one stage earlier, so its 8-bit answer is
today's, and the top byte of what it stores is today's byte. Where a stage
keeps more than a byte from one pixel to the next — an image run's
accumulators, a pyramid's levels — it keeps today's state beside its 16-bit
state, so its 8-bit answer is still today's. A value entering from outside
the canvas — a resolved colour, an operation's alpha, a coverage, a sample —
enters as `q16` of the value today's byte was rounded from, or `widen` of
today's byte where there is nothing finer to take.

### The formats and their byte order

**Big-endian, two bytes a component**, in the component order of the 8-bit
format, alpha last where the 8-bit format has it last. The top byte comes
first, so a 16-bit row's even bytes are the 8-bit row; it is the order PNG
(ISO/IEC 15948 7.1), PSD and a TIFF marked `MM` store, so FO-03 to FO-05
write a row as it stands; and a canvas stores bytes rather than host `u16`s,
so no target reads its samples in a different order (ruling 4).

| 8-bit format | 16-bit counterpart | Role |
| --- | --- | --- |
| `Gray8` | `Gray16` | a page format |
| `GrayA8` | `GrayA16` | a page format; a grey group's, soft mask's and cell's buffer |
| `Rgb8` | `Rgb16` | a page format |
| `Rgba8` | `Rgba16` | a page format; an RGB group's buffer, and every buffer a page asked for in `Rgba16` opens |
| `CmykA8` | `CmykA16` | a `/DeviceCMYK` group's buffer, and an ink page with `RenderOptions::allow_cmyk`, as at eight bits |
| `LabA8` | `LabA16` | a `/Lab` group's buffer, never a page format; its encoding, `L/100`, `(a + 128)/255`, `(b + 128)/255`, taken at sixteen bits by `q16` |
| `Bgr8` | `Bgr16` | an **output order only**: a page asked for in it composites in `Rgb16` and is reordered once, at extraction, each sample's two bytes moving together |
| `Bgra8` | `Bgra16` | an output order only, as `Bgr16`, composited in `Rgba16` |
| `Plates { spots }` | `Plates16 { spots }` | two bytes a tint, `2·(5 + spots)` a pixel: the separations' and overprint simulation's plates, so a 16-bit page simulating overprint composites its inks at sixteen bits |

`PixelFormat::components` keeps counting components. Beside it,
`PixelFormat::sample_bytes` is one or two and `PixelFormat::pixel_bytes`
their product, and every stride and every index into a canvas, a `Bitmap` or
the toolbox's `Layout` is in `pixel_bytes`. `Bitmap::components`, whose
documentation says bytes because the two were the same number, says
components, and `Bitmap::pixel_bytes` stands beside it. `Color16` stands
beside `Color`: red, green, blue and alpha at sixteen bits, with
`Color16::widen(Color)`.

### Coverage

The filler accumulates 4 096 units a pixel — sixteen sub-scanlines of 256
units — and today's mask value is `min(floor(units / 16), 255)`. **A pixel's
16-bit coverage is `min(16·units, 65 535)`**: its top byte is today's value
for every unit count, the cap included, and its low byte the units below a
sixteenth of a byte level, so it is (A) with nothing held. 4 096 units fit a
16-bit word exactly, and (B)'s `round(65 535·units / 4 096)` is `16·units − 1`
above 2 048 units, whose top byte is a level below today's at each multiple
of sixteen between there and the whole pixel: the first place the two
candidates part.

A wide mask's products — a clip's, a soft mask's, `intersect` and
`intersect_in_place` — are `hold(mul16(a, b), product(a8, b8))`; `uniform`
and `overwrite` take a 16-bit value; a hardened or quantised mask
(`harden`, `quantise`) is its 8-bit values widened, since the reduction
discards the precision on purpose. The units are the page frame's, so a tile
measures what the page measures (ruling 5).

### The compositing core

An operation's alpha is `q16(alpha)`, which is `hold(round(65 535·α),
operation_level(α))`. `Opacity`'s levels — a pixel drawn directly, its
coverage, its sample's alpha and the clip; a pixel composited from a run, the
run's coverage and the clip — are held to today's `level_three` and
`level_two`: at an alpha of one the integer quotient of the three 16-bit
factors over `65 535²` (or of two over 65 535), rounded, and at any other the
`f64` expression in today's order with each factor over 65 535, rounded by
`nearest_level` at 65 535, as at eight bits.

`blend` composites straight colour (11.3.6), and each line of it that yields
a byte yields, at sixteen bits, the same formula over `0..=65 535` with
`mul255` as `mul16` and 255 as 65 535, held to that line's byte:

- the blend function's answer `mixed` (the separable and non-separable modes
  below), with a subtractive channel complemented as `65 535 − c`;
- the weighted `blended` channel, `mul16(65 535 − ab, cs) + mul16(ab, mixed)`;
- the backdrop's alpha, its own and, in a non-isolated group, the union
  `own + mul16(initial, 65 535 − own)`;
- **the opaque branch**, `min(mul16(mixed, a) + mul16(cb, 65 535 − a),
  65 535)`, today's fast path when the backdrop's alpha is full;
- **the straight-alpha branch**, `min((w·65 535 + u / 2) / u, 65 535)` with
  `w = mul16(mixed, a) + mul16(mul16(cb, ab), 65 535 − a)` and the union
  `u = a + mul16(ab, 65 535 − a)`, zero where `u` is;
- the alpha channel, `min(a + mul16(own, 65 535 − a), 65 535)`.

The branch taken is the one today's arithmetic takes on the top bytes, so a
16-bit pixel whose backdrop alpha is 65 280 to 65 534 — `255` in its top byte
— composites by the opaque branch, as the 8-bit pixel under it does, and holds
there. `blend_plates` is the same arithmetic tint by tint, with today's rule
for which channels a mode reaches. The opaque blocks of `fill_opaque` and
`over_opaque` are the opaque branch over a run of pixels.

### The sixteen blend modes

`BlendMode::apply16` and `apply_nonseparable16` are 11.3.5's formulas over
`0..=65 535`, each answer held to `apply` and `apply_nonseparable` over the
top bytes, so a 16-bit canvas blends as the clause states and tops out at
today's byte:

- the separable modes with `mul16` for `mul` and 65 535 for 255; `HardLight`
  branches at `cs ≤ 32 767`, where today's branches at `cs ≤ 127`, both the
  clause's `cs ≤ 0.5`; `ColorDodge` and `ColorBurn` divide at sixteen bits in
  `u64`, a zero divisor the formula's own limit as today;
- `SoftLight`'s `D(x)` with its cubic's constants at 65 535 and
  `isqrt(x·65 535)`, integer Newton as today;
- `Lum` with the clause's 0.3, 0.59 and 0.11 as 300, 590 and 110 over 1 000,
  never `Color::luma`'s weights; `ClipColor`, `SetLum`, `Sat` and `SetSat`
  in `i64`, bounded by 65 535. The non-separable branch of `blend` keeps its
  loops bounded by the channel count, for the reason its comment gives.

A subtractive buffer's non-separable modes go through light as today:
8.6.4.4's relation out of ink at sixteen bits, the mode, and maximum
undercolour removal back, each held to today's byte through the same round
trip.

### Groups, soft masks and knockout

The renderer opens a group's buffer, a soft mask's group, a tiling cell and
a mesh's buffer in the 16-bit counterpart of the format it opens today:
`alpha_format` maps each 16-bit format as it maps its 8-bit one, and
`group_format` takes the page's depth beside the group's space, so a `/Lab`
group of a 16-bit page composites in `LabA16` and a `/DeviceCMYK` one in
`CmykA16`.

- `adopt_backdrop` and `hold_parent` copy two bytes a component;
  `remove_backdrop`'s 11.4.7.2 division, `C − (C0 − C)·(α0/αg − α0)` in
  today's integer form, is computed at sixteen bits and held to today's byte;
  `knock_out` and `recolor` are the same rules at sixteen bits.
- `composite` and `composite_tinted` put a group onto its parent through
  `blend` at sixteen bits.
- `Canvas::to_mask` reads a luminosity soft mask as `Lum` at sixteen bits and
  an alpha one as the 16-bit alpha, each held to today's mask byte. **A soft
  mask's `/TR` is a 65 536-entry table at sixteen bits**: entry `S` is
  `hold(round(65 535·f(S / 65 535)), table8[S >> 8])`, `f` evaluated through
  `tinker-pdf-math` as today's 256 entries are, so the transfer is exact at
  every 16-bit input and tops out at today's table. 128 KiB a soft mask with a
  transfer function, made once, as today's table is.

### Images and image runs

`ImageSource` takes 16-bit samples beside its bytes: an 8-bit image's are
widened, a `/BitsPerComponent 16` image's and a JPEG 2000 codestream's above
eight bits are their own (RC-07.12), and a twelve-bit JPEG's are its samples
scaled to sixteen (RC-07.13); in each case the 8-bit samples the twin
evaluation reads are the ones today's render reads, so where today narrows a
sample to its high byte the 16-bit sample is held to that byte.

- The sampler's integer weights — bilinear's two taps a dimension, the box
  filter's coverage of each sample — are today's, applied to 16-bit samples
  in `u64` and divided as today divides, each channel held to today's.
- `Pyramid` halves 16-bit levels as it halves 8-bit ones, holding each level
  to today's level, which it keeps beside.
- `Fragments` accumulates a run at sixteen bits: premultiplied colour and
  coverage totals of two bytes, beside today's four bytes. `fragments.rs`'s
  bound carries over unchanged in form: coverage totals are held to 65 535,
  so the sum of `colour × coverage / 65 535` over a pixel's fragments is at
  most 65 535 as well, and a run's pixel reaches the canvas once, held to
  today's.

### Meshes and shadings

`draw_mesh` interpolates a vertex colour across a triangle by today's integer
barycentric weights over 16-bit vertex colours and composites a 16-bit
colour, held to today's at each pixel. `Shading::color_at` evaluates types 1
to 3 through the same `tinker-pdf-math` evaluation as today and quantises its
components by `q16` (RC-07.11); a mesh's colour closure hands the sampler
`q16` of the same values. Ruling 22 bounds a shaded pixel's top byte, which
is today's byte; where P-14's sampled function tables land, they carry a
16-bit output beside their bytes, held to them.

### Resolved colour

`tinker-pdf-color` answers every space at sixteen bits beside its bytes
(RC-07.7): `ColorSpace::to_rgb16_in` evaluates the components exactly as
`to_rgb_in` does — the device relations, the CIE-based closed forms,
`/Indexed` through its base, `/Separation` and `/DeviceN` through their
alternates, an ICC transform to its `f64` components — and quantises each
component by `q16` against the byte `to_rgb_in` gives, so an ICC colour is
one evaluation quantised twice, never a second transform. `lab_to_srgb16` and
`srgb_to_lab16` are the same: one evaluation, two quantisations.

The interpreter carries a 16-bit colour beside each 8-bit one (RC-07.10):
`GraphicsState` keeps `fill_color` and `stroke_color` as they are, every
8-bit reader takes the bytes it takes today, and `fill_wide` and
`stroke_wide` hold the 16-bit light; the document's own ink and a colour's
colorant tints carry their 16-bit counterparts the same way. `FontSource`
gains `resolve_color_wide`, answering a colour's bytes and its 16-bit light
together; its default widens `resolve_color`'s answer, a correct 16-bit
colour that refines nothing, so a test source that overrides only
`resolve_color` keeps working, and the facade's `PageResources` overrides it
through `to_rgb16_in`, evaluating each colour once.

### The page, the facade, and the output until FO-05

`RenderOptions::format` accepts `Gray16`, `GrayA16`, `Rgb16`, `Rgba16`,
`Bgr16` and `Bgra16`, and `CmykA16` with `allow_cmyk`; `page_format` hands
`LabA16` and `Plates16` back as `Rgba16`, as it hands their 8-bit forms back
as `Rgba8`, and `canvas_format` composites the blue-first orders red first.
`Page::render`, `render_form`, `render_annotation` and `DisplayList::render`
all go through `render_layer_with`, so the one change reaches each. A 16-bit
`Bitmap` is the format's samples in the byte order above, and `premultiplied`
multiplies at sixteen bits, held to today's premultiplication. A tile is the
same pipeline over a smaller canvas in the page's frame, so a 16-bit tile is
the 16-bit page under it (ruling 5).

- **`Bitmap::to_png`** writes a 16-bit bitmap's top bytes as the 8-bit
  format's PNG — by (A) the 8-bit render's file, byte for byte — until FO-05
  adds the option that writes its sixteen bits. The table in its
  documentation gains a row for each 16-bit format.
- **`pdfcmp`** renders at `Rgb8` and compares eight bits, which is the scale
  its budgets are written in; a 16-bit PNG it reads is narrowed by
  `Bitmap::from_png` as today.
- **`tpdf`** keeps asking for 8-bit formats; a 16-bit `render` is FO-05's,
  with the file it writes.
- **The C ABI and the bindings** keep their four 8-bit formats until SU-12
  projects the facade's.

A 16-bit page costs twice an 8-bit page's buffers, and every buffer a render
opens doubles with it. The twin arithmetic computes each stored sample twice,
so a 16-bit render costs more than twice; RC-07.16 records by how much. An
8-bit render runs none of it: the 8-bit code is today's, and the 16-bit paths
are reached only from a 16-bit canvas.

## Milestones

1. **RC-07.2, the price of each relation.** `crates/tinker-pdf-raster/tests/
   sixteen_bit_census.rs` (new):
   `the_sixteen_bit_candidates_priced_against_today_s_arithmetic`,
   `#[ignore]`d, sweeps every 8-bit coverage, source, backdrop and constant
   alpha through both of `blend`'s backdrop branches and the filler's 4 096
   coverage units, takes today's byte from `Canvas` and each candidate's
   16-bit value from arithmetic in the test, and prints, per candidate, how
   many top bytes differ from today's, how many values (A) holds, and how far
   each lies from the exact value. Run with `cargo test --release -p
   tinker-pdf-raster --test sixteen_bit_census -- --ignored --nocapture`; it
   prints its `RAN` banner and the figures as tab-separated lines, which are
   committed as `crates/tinker-pdf-raster/tests/sixteen_bit_census.tsv`
   beside that command when it runs (ruling 16). Each stage is priced on its
   own, its 8-bit operands widened: the effective alpha over every coverage
   and constant alpha, the opaque branch over every effective alpha, source
   and backdrop, the straight-alpha branch over every effective alpha and
   backdrop alpha short of opaque on a grid of sources and backdrops a
   seventeenth apart, and coverage over every unit count.
2. **RC-07.3, the formats and the core.** The table's formats, `Color16`,
   `sample_bytes` and `pixel_bytes`; `Canvas` stores, clears, reads and
   composites them through `blend`'s `Normal` path and both backdrop
   branches. Exit: `canvas.rs`'s
   `a_sixteen_bit_composite_tops_out_at_the_eight_bit_composite` over the
   domain RC-07.2 swept, `a_sixteen_bit_low_byte_is_the_design_s_closed_form`
   computed in the test from the forms above, and
   `a_sixteen_bit_canvas_extracts_to_every_format`; every 8-bit `canvas.rs`
   test, `transparency_groups.rs` and `determinism.rs` pass unchanged.
3. **RC-07.4, the blend modes.** Exit: `blend.rs`'s
   `every_mode_at_sixteen_bits_tops_out_at_its_eight_bit_value` over every
   pair of 8-bit operands widened, and
   `the_sixteen_bit_modes_follow_11_3_5_s_formulas` against the clause's
   formulas evaluated in the test at 16-bit operands; `blend_modes.rs` and
   the `blend` and `analytic_blend` fingerprints pass unchanged.
4. **RC-07.5, coverage.** Exit: `fill.rs`'s
   `wide_coverage_tops_out_at_the_eight_bit_mask` over every unit count from
   0 to 4 096 and the products of two; `analytic_coverage.rs`'s
   `a_half_covered_edge_at_sixteen_bits_is_the_units_the_grid_states` and
   `a_corner_at_sixteen_bits_is_the_product_of_its_fractions`; every 8-bit
   mask test passes unchanged.
5. **RC-07.6, the sampled paints.** Exit: `analytic_sampling.rs`'s
   `minification_at_sixteen_bits_is_the_box_average` at the file's six ratios
   and `abutting_strips_at_sixteen_bits_leave_no_backdrop`; `mesh.rs`'s
   `a_sixteen_bit_mesh_tops_out_at_the_eight_bit_mesh`; every 8-bit
   sampling, run and mesh test passes unchanged.
6. **RC-07.7, colour at sixteen bits.** Exit: `tinker-pdf-color`'s
   `every_space_at_sixteen_bits_tops_out_at_its_eight_bit_colour` over a grid
   of components per space, and
   `device_spaces_at_sixteen_bits_are_their_relations` (8.6.4.4 for CMYK, the
   components themselves for grey and RGB) computed in the test; the crate's
   tests and `colour_spaces.rs` pass unchanged.
7. **RC-07.8, the renderer on a 16-bit page.** Exit: `tinker-pdf-render`'s
   `a_sixteen_bit_page_tops_out_at_the_eight_bit_page` over an isolated and a
   non-isolated group, a knockout group, a luminosity and an alpha soft mask
   and a tiling pattern, each rendered at both depths; every render-crate
   test passes unchanged.
8. **RC-07.9, the facade at sixteen bits.** Exit: `sixteen_bit.rs`'s
   `a_sixteen_bit_render_tops_out_at_the_eight_bit_render` over
   `render_support`'s analytic pages and blend grid, and
   `the_low_byte_of_each_arithmetic_fixture_is_its_closed_form` (a
   half-covered edge, a half-alpha fill, a group at half alpha, the blend
   grid) computed in the test; `render_regions.rs`'s
   `a_sixteen_bit_tile_is_the_sixteen_bit_page_under_it`; `render_options.rs`
   and `png_output.rs` unchanged; both builds.
9. **RC-07.10, the resolved colour.** Exit: `tinker-pdf-content`'s
   `a_resolved_colour_keeps_sixteen_bits_and_its_eight_bit_bytes` — `g`,
   `rg`, `k`, `sc` and `scn` through a test resolver record the 16-bit colour
   and the bytes they record today; `display_list.rs`, `colour_spaces.rs` and
   `determinism.rs` pass unchanged.
10. **RC-07.11, colour painted at its precision.** Exit: `sixteen_bit.rs`'s
    `a_flat_colour_keeps_its_low_byte` (DeviceGray, DeviceRGB, DeviceCMYK, ICC
    and `/Lab` fills, each 16-bit value computed from the components in the
    test) and `a_gradient_ramps_at_sixteen_bits` (`render_support`'s axial
    and radial pages, every pixel computed in the test);
    `render_analytic.rs`, `mesh_shadings.rs` and the shading fingerprints
    pass unchanged; both builds.
11. **RC-07.12, images keep their precision.** Exit: `sixteen_bit.rs`'s
    `a_sixteen_bit_image_keeps_its_samples` and
    `a_high_precision_jpx_keeps_its_samples`, each drawn 1:1 on a 16-bit page
    and returning its samples through its colour space; `images.rs`,
    `jpx_images.rs`, `inline_images.rs` and the `image` and `jpx`
    fingerprints pass unchanged.
12. **RC-07.13, twelve-bit JPEG.** Exit: `jpeg.rs`'s
    `a_twelve_bit_frame_hands_back_twelve_bits_when_asked`; `sixteen_bit.rs`'s
    `a_twelve_bit_jpeg_keeps_its_precision_on_a_sixteen_bit_page`;
    `jpeg_census.rs` and the decoder's tests pass unchanged.
13. **RC-07.14, the row's exit over the fingerprints.** Exit: `determinism.rs`'s
    `every_fingerprint_s_sixteen_bit_render_tops_out_at_its_eight_bit_render`
    in the default and `bundled-fonts` builds, each fixture's ink floor and
    `UnreadableFont` check applied to the 16-bit render too.
14. **RC-07.15, determinism at sixteen bits.** Exit: `determinism.rs`'s
    `sixteen_bit_rendering_is_stable_across_targets`, the SHA-256 of each
    fixture's 16-bit render in the byte order above, pinned on four targets
    and entered in `xtask/src/determinism.rs`'s roster.
15. **RC-07.16a and RC-07.16b, the speed rows.** MS-04's text page with an
    embedded face at 150 dpi and its transparency fixture, each at sixteen
    bits, join the render group beside their 8-bit operations; RC-07.16b
    takes the figures.

## Risks

| Risk | Mitigation |
| --- | --- |
| A 16-bit path drifts from the 8-bit one and its top byte stops being today's | The 8-bit answer of each stage is today's function called on the top bytes, never a transcription; RC-07.3 to RC-07.9 and RC-07.14 hold every top byte to today's render, the last over every fingerprint fixture |
| The low byte is held at an interval's end where the 16-bit rounding falls outside it, so it refines nothing there | Stated rather than hidden: a held value is within half an 8-bit level of the exact one, and RC-07.2 counts how often a value is held and how far it lies |
| An 8-bit fingerprint moves | Nothing on an 8-bit canvas changes: the 16-bit paths are reached from 16-bit formats alone, and every existing test and fingerprint is in each step's exit unchanged |
| A wider operand overflows: `mul16`'s product, `ColorDodge`'s and `ColorBurn`'s quotients, `ClipColor`'s and `remove_backdrop`'s arithmetic, `Opacity`'s three factors | Each is `u64` or `i64` at sixteen bits, its bound argued where it is written as `fragments.rs` argues its own; the census and the closed-form tests reach the ends of every range |
| A stride computed in components rather than bytes reads half a 16-bit row | `pixel_bytes` is the one stride; `components` keeps meaning components, and `Bitmap::components`' documentation says so |
| Doubled buffers hold twice the memory under the same budgets | `MAX_PAGE_PIXELS`, `MAX_IMAGE_RUN_PIXELS` and `MAX_GROUP_BUFFERS` count pixels and buffers, which a 16-bit render keeps, so it holds twice an 8-bit render's bytes under each; RC-07.8 keeps the counts and says so beside each, and a page past one degrades as an 8-bit one does (ruling 2) |
| A 16-bit PNG FO-05 writes is read back by `Bitmap::from_png`, whose rounding to eight bits is not the top byte where a value was held at its interval's top | FO-05's own exit compares its samples with RC-07's render; reading 16-bit files into 16-bit bitmaps is FO-05's to decide |
| A target reads samples in its own byte order | A canvas stores bytes in the one order above and reads them with `from_be_bytes`; RC-07.15 pins the hashes on four targets |
