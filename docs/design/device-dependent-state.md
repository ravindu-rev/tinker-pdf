# Device-dependent graphics state, applied

*For [ROADMAP](../ROADMAP.md) row RC-08.* K-01 reads every key of a graphics
state parameter dictionary and stores the device-dependent ones in
`GraphicsState::stored` ([graphics-state](graphics-state.md)), where they move
no pixel. This design says where each enters the pipeline: the transfer
functions, black generation and undercolour removal, halftones and their
origin, flatness, smoothness, stroke adjustment, the alpha source and text
knockout. Each is settled from ISO 32000-2's clause for it — Table 57 (Table
58 in ISO 32000-1) and the clauses its entries cite, 10.4 to 10.7, 9.3.8 and
11.3.7, 11.4.6, 11.6.4 and 11.7.4 to 11.7.5 — with the fixture whose effect
its test computes. Overprint and plates are RC-03's and RC-04's, the 1-bit
outputs RC-06's ([output-pixel-models](output-pixel-models.md)).

## Scope

- **`/TR`, `/TR2`**: which device components, at which stage, in and out of
  transparency (10.5, 11.7.5.2).
- **`/BG`, `/BG2`, `/UCR`, `/UCR2`** in the conversion of light to ink
  (10.4.2.4, 11.7.5.3).
- **`/HT`, types 1, 5, 6, 10 and 16, and `/HTO`** on RC-06's 1-bit output,
  per point (10.6, 11.7.5.2).
- **`/FL` and `i`** as the flattening tolerance, **`/SM`** as a shading's
  colour tolerance (10.7.2, 10.7.3).
- **`/SA`**'s rule (10.7.5).
- **`/AIS`**'s split of shape from opacity (11.3.7, 11.6.4), and knockout
  made exact (11.4.6).
- **`/TK`** over a text object (9.3.8), and the combined fill and stroke 11.7.4.4
  paints as one knockout element.
- The value every key takes where the document leaves it unset — **the
  device's own**, which is what lets each step say that a page without its
  key moves no byte.
- RC-08.2 to RC-08.12's exit tests.

## Non-goals

- **Overprint** (`/OP`, `/op`, `/OPM`) and 11.7.4.3's compatible blend mode —
  RC-03, which the plane below gives per-component screens when overprinting
  is on (11.7.5.2's last paragraph).
- **Separations and spot plates** — RC-04. A halftone or transfer function
  named for a spot colorant in a type 5 halftone applies to its plate once
  plates are screened, which no step here does.
- **SVG output.** It is device-independent by construction; 10.5 says "a page
  description that is intended to be device-independent should not define a
  current transfer function". The writer emits colours as the content set
  them, with none of these keys.
- **A device's named halftones** (`/HalftoneName`): no device carries any, so
  10.6.5.1's fallback — the dictionary's own entries, else the default — is
  what is read, as K-01 stores it.
- **Accurate screens** (`/AccurateScreens`): ordinary halftoning, which
  10.6.5.2's NOTE 1 says "adjusts them so that a single screen cell is
  quantised to device pixels".
- **ISO 32000-2's deprecations** of `/TR` and `/TR2`: they are applied as
  written; a warning for them is SD-18.6's.

## Design

### The device's own value

Table 52 starts black generation, undercolour removal, the transfer function,
the halftone and smoothness at "a suitable device dependent value", and
flatness at 1.0; Table 51 starts stroke adjustment and the alpha source
false. K-01 stores an unset key, and `/Default`, as `None` — "the device's
own" (`StoredParameters`) — so each key has a device value here, chosen as
what the renderer does today:

| Parameter | The device's own |
| --- | --- |
| transfer | the identity |
| black generation and undercolour removal | the conversion an ink canvas makes today, maximum undercolour removal (`rgb_to_cmyk`) |
| halftone | RC-06's default screen, the 16 × 16 dispersed-dot matrix, at the page frame's origin |
| flatness | 0.2 device pixels (`Renderer::tolerance`) |
| smoothness | none: a patch is subdivided by device length alone |
| stroke adjustment | off |

Flatness is the one whose clause gives a number: Table 52's initial 1.0. 10.7.2
lets a processor "choose to ignore any flatness tolerance specified within a
PDF file", and where the content sets none the renderer keeps its own 0.2 —
finer than 1.0, so no curve is coarser than the initial value allows, and every
page that sets none keeps its bytes. A flatness the content sets is honoured,
as below. Text knockout and the alpha source are plain fields (9.3.8 starts
`/TK` true, Table 51 `/AIS` false), so they have no device value; the readings
below keep today's bytes for every page whose fields hold those starting
values.

### The rendering-parameter plane (11.7.5.2)

Transfer functions and halftones are the first category of 11.7.5.1: "applied
only when the final colour at a given point on the page is known". 11.7.5.2:
"The halftone and transfer function to be used at any given point on the page
shall be those in effect at the time of painting the last (topmost)
elementary graphics object enclosing that point, but only if the object is
fully opaque … For portions of the page whose topmost object is not fully
opaque or that are never painted at all, the default halftone and transfer
function for the page shall be used." So they are kept per point, not applied
per object:

- **The plane** is one byte per device pixel of the page canvas, an index
  into a table of the distinct parameter sets the render met — a transfer, a
  halftone and whether the colour was DeviceGray (below) — whose entry 0 is
  the device's own. It is made at the first paint whose set is not entry 0, so
  a page that sets none of these keys keeps no plane and moves no byte. Past
  255 distinct sets a page's later ones are recorded as entry 0 and named,
  `RenderWarning::DeviceParametersSpent` (ruling 2 and 10).
- **Each elementary object writes it where its shape is not zero**: its own
  set's index if it is fully opaque, entry 0 if it is not. Painting order is
  stacking order, so the plane holds the topmost object at every pixel. A
  group's result writes nothing ("Only elementary objects shall be relevant;
  the rendering parameters associated with a group object are ignored"), and
  neither does a soft mask's group, which paints no page.
- **Fully opaque**, as 11.7.5.2 lists it: the constant alpha for the paint
  (`CA` for a stroke, `ca` otherwise) is 1, the blend mode `Normal`, no soft
  mask; an image with no `/SMask` (nor `/SMaskInData`); every group enclosing
  it was invoked under the same three conditions (each `GroupFrame` records
  whether it was); and a tiling pattern's colour only if every paint of its
  cell was fully opaque (the cell's renderer reports it on its `Tile`).
- **The shape** is the object's own (11.6.4.2), clipped, since the clip is
  what limits a paint on the page: a path's or a glyph's coverage times the
  clip, without the soft mask, which is opacity; an image's unit square times
  its stencil or colour-key `/Mask` and the clip — drawn once more through the
  sampler into an alpha probe with its colours ignored, because the sampler is
  what decides each pixel's share of a sample — and not its `/SMask`, which is
  opacity; a shading under `sh` where it defines a colour, within the clip; a
  mesh's coverage; a pattern's fill where its cells or its shading painted,
  within the shape.
- **At the end of `render_layer_with`**, after the output conversion
  ([output-pixel-models](output-pixel-models.md), step 4), each pixel's
  components go through its entry's transfer; RC-08.9's screening reads its
  entry's halftone. Each decision reads one pixel and the paints over it, all
  in device pixels of the page, so a tile records exactly the page's plane over
  its rectangle (ruling 5).

### Transfer functions (`/TR`, `/TR2`, 10.5)

- **The value in force** is `/TR2` over `/TR`, as K-01 reads it: one function,
  four, `/Identity`, or `/Default`, the device's own.
- **On the output's own components.** 10.5: "Transfer functions shall always
  operate in the native colour space of the output device … An RGB device
  shall use the first three, a monochrome device shall use the gray transfer
  function only, and a CMYK device shall use all four"; one function serves
  every component. So: `Rgb8`, `Rgba8`, `Bgr8`, `Bgra8` — red, green and blue
  through the first three; `Gray8`, `GrayA8` and a 1-bit render's grey —
  through the fourth; the ink page (`CmykA8` with `allow_cmyk`) — C, M, Y and K
  through all four; a destination (RC-06.5) — its device components by its data
  space, the same way. Alpha never.
- **In additive form.** "If the component is subtractive … it shall be
  converted to additive form by subtracting it from 1.0 before it is passed to
  the transfer function. The output of the function shall always be in
  additive form": an ink byte `c` becomes `255 − T(255 − c)`.
- **DeviceGray on a CMYK output** (10.5's special case): where the plane's
  topmost object was specified in DeviceGray, only the fourth function is
  applied, to K; C, M and Y "shall not be passed through their respective
  transfer functions but are rendered directly". That is why the plane's sets
  carry the colour's family.
- **A halftone's `/TransferFunction`** overrides the state's for its component
  (10.5) where that halftone screens — RC-08.9's screening, on the 1-bit
  output. A continuous-tone output uses no halftone (10.6.1), so neither does it
  use a halftone's transfer.
- **Sampled once, applied in integers** (ruling 4): each distinct function is
  sampled to 256 entries the first time a set holds it (`sample_transfer`'s
  form), and a pixel is a lookup per component.

### Black generation and undercolour removal (10.4.2.4, 11.7.5.3)

- **Only where DeviceRGB becomes DeviceCMYK.** 11.7.5.3: they "shall be
  applied only during conversion from DeviceRGB to DeviceCMYK colour spaces",
  and say which functions: "When painting an elementary object with a
  DeviceRGB colour directly into a transparency group whose colour space is
  DeviceCMYK, the functions used shall be the current … functions in effect in
  the graphics state at the time of the painting operation"; "When painting a
  transparency group whose colour space is DeviceRGB into a parent group whose
  colour space is DeviceCMYK, the functions used shall be the ones in effect at
  the time the `Do` operator is applied to the group"; and the page group in
  DeviceRGB on a CMYK device takes "the default functions for the page". So
  they apply on an ink buffer — the ink page, and a `/DeviceCMYK` group's buffer
  on any render — to a paint whose colour is of the DeviceRGB family, and to an
  RGB group closed onto an ink buffer, with the functions its frame recorded at
  the `Do`.
- **Every other colour converts as it does today.** A DeviceCMYK colour is its
  own ink. A DeviceGray colour is 10.4.2.3's `0 0 0 1−g`, which is what
  maximum removal makes of a grey's light, byte for byte. A CIE-based colour
  reaches an ink buffer through 10.3's conversion, for which the engine's
  reading is maximum removal of its light, unchanged.
- **The family travels with the colour.** `GraphicsState` gains
  `fill_family` and `stroke_family`, a `ColorFamily` — `Gray`, `Rgb`, `Cmyk`
  or `Other` — set where the interpreter resolves a colour: by `g`, `rg` and `k`
  themselves, by count where `sc` has no space named, and through the seam's
  `FontSource::color_family` for a space by name — an `/Indexed` space its
  base's, a `/Separation` or `/DeviceN` its alternate's, as 11.7.4.3's NOTE 2
  reads a "current colour space", and a device space a default replaces its
  substitute's. 8.6.8's initial space is DeviceGray, so both start `Gray`. An
  image's family rides on `DecodedImage`, a shading's is read off its
  `ColorSpace`. RC-03.5's colour kind, which overprint needs, extends the same
  field.
- **10.4.2.4's formula, in integers**: with `r`, `g`, `b` the light's bytes,
  `k = 255 − max(r, g, b)`, each ink `clamp(255 − r − UCR[k])` and the black
  `clamp(BG[k])`, `BG` sampled to `0..=255` and `UCR` to `−255..=255`
  (Table 57: it "maps the interval [0.0 1.0] to the interval [−1.0 1.0]").
- **The device's own** is today's: with both functions unset the conversion is
  `rgb_to_cmyk`, byte for byte. With one set, the other is `k` itself — 10.4.2.4's
  own words for "return its k operand unchanged" — since maximum removal
  written in 10.4.2.4's terms is that pair.
- **In the leaf**, an ink canvas carries its conversion of light into ink as a
  plain value — a pair of tables, `InkTables`, put in force by
  `Canvas::set_ink_tables`, or none for maximum removal — which
  `Canvas::encode` reads for `CmykA8`. The renderer sets it for a paint, and for
  a group's composite, and puts the device's own back after; a flat colour, a
  shading's pixel, an image's sample and a group's pixel all become ink there,
  so one conversion serves every path. RC-05's CMYK proof sets the proof's
  device leg in the same place.

### Halftones and their origin (`/HT`, `/HTO`, 10.6)

- **Only a 1-bit output screens.** 10.6.1: "Some output devices can
  reproduce continuous-tone colours directly. Halftoning is not required for
  such devices". A light, ink or destination render ignores `/HT`.
- **Per point, by the plane.** `Screening::Halftones` (RC-08.9) compares each
  pixel's grey with the threshold of its entry's halftone at that device pixel,
  the default screen where the entry is 0; RC-06's `OrderedDither` and
  `ErrorDiffusion` read no document halftone (10.6.5.1 lets a processor use
  halftones "better suited for that device").
- **One screening path.** Every halftone becomes a `tinker_pdf_raster::halftone`
  cell ([output-pixel-models](output-pixel-models.md)): type 6 its `Width` by
  `Height` bytes; type 16 its 16-bit rectangle, or two tiled as 10.6.5.5's
  Figure 68 tiles them; type 10 its two squares, `Xsquare²` then `Ysquare²`
  bytes, tiled as 10.6.5.4's figures tile them — by the lattice whose vectors are
  the displacement between corresponding points of adjacent cells, `(X, Y)`,
  and its quarter turn; type 5 the entry for the output's colorant — `/Gray` on
  a grey device (10.6.5.6) — or its `/Default`; type 1 the cell RC-08.10 builds.
  RC-08.8 states each reduction from a device pixel to its threshold and holds it
  to a direct tiling at every pixel of a period. A threshold of 0 reads as 1
  (10.6.4).
- **Type 1's cell** (10.6.5.2): its side `s = resolution / frequency` device
  pixels at the render's resolution (72 × the scale), turned by `/Angle`
  counterclockwise in device space, quantised to whole pixels as type 10's cell:
  `X = round(s cos θ)`, `Y = round(s sin θ)`, the sine and cosine from
  `tinker-pdf-math` (the facade is not on `PIXEL_PATHS`, ruling 4). Each pixel of
  the two squares is mapped to the cell's own coordinates in −1 to 1 (10.6.3),
  the spot function evaluated there — a Table 126 name, the first a `/SpotFunction`
  array names that Table 126 holds, or a function — and the pixels ranked by
  value, ties in raster order (10.6.3 leaves their order "arbitrarily", which is
  not an order a fingerprint can hold). Rank `r` of `n` takes the threshold
  `ceil((r + 1) × 65 535 / n)`, so a flat grey `g` whitens `floor(g × n)` of
  them, 10.6.2's count. A cell past 2²⁰ pixels is refused for the default screen
  and named (`RenderWarning::UnsupportedHalftone`).
- **`/HTO`** (PDF 2.0): "specified in the current coordinate system … changes to
  the current coordinate system … do not move the halftone origin relative to the
  underlying device coordinate system". So it is mapped through the CTM in force
  at the `gs` when it is set: RC-08.9 stores the point in default user space
  rather than the numbers as written, and the renderer maps it through the page
  transform into device pixels of the whole page. Absent, the origin is the page
  frame's.
- **A halftone K-01 stored as absent** because it was malformed screens as the
  device's own (ruling 2), and a threshold stream that decodes short is refused
  for the default screen and named, where it is decoded.

### Flatness (`/FL`, `i`, 10.7.2)

- **The tolerance is the flatness in device pixels**: "the maximum permitted
  distance in device pixels between the mathematically correct path and an
  approximation constructed from straight line segments". Table 56's operand is
  "a number in the range 0 to 100; a value of 0 shall specify the output
  device's default flatness tolerance", so 0 and a negative number are the
  device's own 0.2, as an unset key is (above); a value above 100 is 100; and
  none is finer than 1/256 of a pixel, the grid the filler places a crossing
  on, below which a closer chord cannot change a coverage.
- **Per paint, from the paint's state**: fills, strokes (the path flattened
  before it is stroked, and the stroker's round joins and caps), clips (`W n`
  flattens at the paint that ends the path, as a fill would, 10.7.4), glyph
  outlines filled and stroked, and a text clip's glyphs, each at the flatness
  in force when it was shown. It is a device-pixel tolerance on device
  geometry, so a tile flattens the page's curves (ruling 5).
- **The flattener's rule is unchanged** — `ceil(1.5 × √(polygon / tolerance))`
  steps, one to 512 — only its tolerance is the state's; `determinism.rs`'s
  pages set no flatness and move no byte.

### Smoothness (`/SM`, 10.7.3)

- **Where the renderer approximates a shading's colour**, and only there:
  "Smoothness is the allowable colour error between a shading approximated by
  piecewise linear interpolation and the true value of a (possibly nonlinear)
  shading function … expressed as a fraction of the range of the colour
  component". Types 1 to 3 are evaluated at every pixel and types 4 and 5
  interpolate their vertices linearly as 8.7.4.5.5 defines them, so neither
  approximates anything `/SM` bounds; ruling 22 bounds their stepping. Types 6
  and 7 interpolate colour bilinearly over a patch (8.7.4.5.7), and the
  renderer draws a patch as triangles over which colour is linear: there the
  error is the bilinear term's, `|c₀₀ − c₀₁ − c₁₀ + c₁₁| × h² / 4` over a step
  `h` of the parameter, for each component and through a function's sampled
  slope where the patch carries one.
- **So under `/SM` a patch takes the larger of** today's device-length count
  and the count whose error is within `/SM` of each component's range, a
  fixed-point decision (`patch_steps`, ruling 4) capped by `MAX_PATCH_STEPS`
  whatever `/SM` asks, chosen in device space for the page frame so a tile
  keeps the page's mesh (`draw_mesh_over`, ruling 5). `/SM` is read in 0 to 1,
  and no finer than one level of 255, below which the error cannot change a
  byte.

### Stroke adjustment (`/SA`, 10.7.5)

- **On, a stroke's width and coordinates are adjusted** "to produce lines of
  uniform thickness. The thickness shall be as near as possible to the
  requested line width — no more than half a pixel different", and a line
  under half a pixel "shall be rendered as a single-pixel line". So the device
  width `w` becomes `W = max(1, round(w))` whole pixels, and every point of the
  device path — vertices and control points alike — is moved to the grid that
  puts the stroke's edges on pixel boundaries: a pixel centre on each axis for
  an odd `W`, a pixel boundary for an even one. A thin line then covers whole
  pixels across its width, at any offset and between its copies; a curve moves
  by under half a pixel and keeps its shape.
- **Where the pen is a device width** — a similarity, nearly every page
  (`user_space_pen`) — and in the page frame, never a tile's (ruling 5). Under
  any other map the coordinates are adjusted and the width is the user-space
  pen's, uniform along the line already. Stroked glyphs take the same rule.
- **Off or unset**, strokes draw as today.

### The alpha source and knockout made exact (`/AIS`, 11.3.7, 11.4.6)

- **Where shape is read.** 11.3.7.2 composes a source's shape and opacity
  into one alpha, so outside a knockout group only their product matters and
  `/AIS` changes nothing. Inside one, 11.4.6 composites each element against
  the group's initial backdrop with its opacity `q` and then weights by its
  shape `f`: `αt = Union(αg0, q)`,
  `Ct = (1 − q)·α0·C0 + q·((1 − α0)·Cs + α0·B(C0, Cs))`, then
  `αg = (1 − f)·αg_prev + f·αt` and the colour likewise, normalised by the
  union alpha. Today `Canvas::knock_out` restores the initial backdrop by
  coverage and the element composites at coverage times alpha — one number for
  both (`knockout_restore`).
- **The split.** A paint's shape is its coverage times the clip; with `/AIS`
  false, its opacity is the constant alpha times the soft mask, and with `/AIS`
  true both are shape: `f` takes them and `q` is 1. An image's `/SMask` follows
  the same flag (Table 57: AIS "also governs the interpretation of the SMask
  entry, if any, in an image dictionary"). A group nested in a knockout group
  is an element whose shape is the union of its elements' shapes, kept in a
  plane beside its buffer only when its parent is a knockout group, the one
  place it is read.
- **What moves.** A knockout group's whole pixels are today's; a pixel an
  element covers partly moves, because 11.4.6 weights by shape after
  compositing at the opacity, where today's restore weights before. RC-08.6
  measures those pixels on the committed knockout fixtures first and puts what
  moves to the owner; it re-pins nothing.

### Text knockout (`/TK`, 9.3.8)

- **On, a text object is "a non-isolated knockout transparency group … where
  each glyph is an individual element"**, composited onto the backdrop "using
  the Normal blend mode and alpha and soft mask values of 1.0", its graphics
  state inherited and not reset. **Off,** "each glyph … shall be treated as a
  separate elementary object", which is what the renderer does for every glyph
  today.
- **Where it differs.** For whole pixels of opaque `Normal` glyphs the two
  agree: the topmost glyph covers what is under it either way. They part where
  glyphs of one object overlap translucently, under another blend mode or a
  soft mask, and at a pixel two glyphs both cover partly. So the renderer takes
  the knockout where it differs for whole pixels: at the first glyph of a text
  object that paints translucently or blended — `Device::begin_text` marks the
  object's start — it snapshots the backdrop, and composites each glyph against
  it through RC-08.6's exact knockout; an opaque `Normal` text object composites
  glyph after glyph as today. That keeps every page of opaque text byte for
  byte, the `text` and `epub` fingerprints included; **its price** is the
  partly covered pixel two opaque glyphs of one object share, composited one
  over the other rather than knocked out — a reading this design states, and
  the owner's to reverse.
- **"Any TK value … installed using the gs operator shall be ignored between
  the BT and ET operators"**: the interpreter applies `/TK` only outside a text
  object, so every glyph of an object carries the value in force at its `BT`.
- **A Type 3 glyph** is one element (9.3.8: the flag "controls the behaviour of
  glyphs obtained from any font type, including Type 3"): inside a knocked-out
  object its procedure paints into a buffer of its own, composited as one
  element.
- **The same knockout, from 11.7.4.4.** The row's "11.7.4.3 knockout case" is
  read here as the knockout compositing this row makes exact: 11.7.4.3 in ISO
  32000-2 is the compatible overprint mode, RC-03's, and the knockout case
  beside it is 11.7.4.4's — a path painted by `B`, `B*`, `b` or `b*`, and a glyph
  in mode 2 or 6, is one object whose fill and stroke form "a non-isolated
  knockout group" when overprinting is off, "independent of the text knockout
  parameter". RC-08.7 composites that pair as one element under the same
  condition — translucent or blended — so the stroke of a translucent glyph
  replaces its fill rather than doubling over it.

## Milestones

1. **RC-08.2, the function keys.** The plane, the transfer functions on every
   output, `ColorFamily`, `InkTables` and 10.4.2.4. Exit:
   `device_state.rs`'s `a_transfer_function_maps_each_component_through_its_curve`
   — a fill, an image and a shading under one function and under four render as
   the page without them with each covered pixel passed through the curves,
   computed in the test, on `Rgb8`, `Gray8` and the ink page; `/TR2` winning;
   `/Identity` and `/Default` moving no byte; a translucent object, one under a
   soft mask, one in a group invoked at `ca 0.5`, and an image with an `/SMask`
   leaving the default where they are topmost; an opaque object over a
   transferred one taking its own; DeviceGray on the ink page transferred on K
   alone; a tile the page's; `black_generation_and_undercolour_removal_make_the_ink_the_test_computes`
   — an `rg` fill, an RGB image, an RGB group and an uncoloured pattern of
   `[/Pattern /DeviceRGB]` on the ink page and in a `/DeviceCMYK` group at
   10.4.2.4's ink, a `g` fill at 10.4.2.3's and a `k` fill at its own;
   `a_glyph_takes_the_transfer_and_the_ink_tables` — a glyph drawn from its
   own mask transferred and inked as a path is; `ext_gstate.rs`'s pins for
   these keys take their pixel halves.
2. **RC-08.3, flatness.** Exit: `device_state.rs`'s
   `flatness_sets_the_tolerance_a_curve_is_flattened_to` — a circle and an
   ellipse, filled, stroked, as a clip and as a glyph, under three `/FL` values,
   each byte-identical to its twin drawing the polygons the test derives from
   the flattener's rule at that tolerance; `5 i` and `/FL 5` twins; 0, a
   negative value and over 100 read as this design reads them; without `/FL` no
   byte moves and `determinism.rs` passes unchanged; `render_regions.rs` holds a
   tile to the page; both builds.
3. **RC-08.4, smoothness.** Exit: `device_state.rs`'s
   `smoothness_bounds_a_patch_s_colour_error`.
4. **RC-08.5, stroke adjustment.** Exit: `device_state.rs`'s
   `stroke_adjustment_gives_a_thin_line_one_width`.
5. **RC-08.6, the alpha source and exact knockout.** Exit:
   `device_state.rs`'s `alpha_is_shape_splits_shape_from_opacity`, the
   measurement of the committed knockout fixtures' partly covered pixels put to
   the owner.
6. **RC-08.7, text knockout and 11.7.4.4's pair.** Exit: `device_state.rs`'s
   `text_knockout_composites_a_text_object_as_one_element` — overlapping glyphs
   at `ca 0.5` and under `/BM /Multiply`, with `/TK` true and false, and a
   translucent mode 2 glyph and `b` path whose stroke knocks out its fill, each
   the composite the test computes per pixel; a `/TK` set between `BT` and `ET`
   ignored; opaque `Normal` text moving no byte; both builds.
7. **RC-08.8, the cell shapes.** Exit: `halftone.rs`'s
   `a_screened_plane_is_its_threshold_comparison_at_every_pixel` and
   `a_screened_tile_is_the_screened_page_under_it`.
8. **RC-08.9, the document's halftones.** Exit: `device_state.rs`'s
   `a_threshold_array_s_dot_pattern_is_the_test_s`.
9. **RC-08.10, type 1.** Exit: `device_state.rs`'s
   `a_spot_function_screen_is_the_cell_the_test_builds`.
10. **RC-08.11, fingerprints.** Exit: `device_state.rs`'s
    `device_dependent_rendering_is_stable_across_targets`, in K-07's roster.
11. **RC-08.12a and RC-08.12b, the speed rows.**

## Risks

| Risk | Mitigation |
| --- | --- |
| A page that sets no key starts paying for these | Every key's unset value is today's behaviour; the plane exists only once a paint sets a non-default transfer or halftone; the conversion of light into ink is today's until a function is in force |
| Transfer applied per object rather than per point, which parts from 11.7.5.2 wherever a translucent object lies over a transferred one | The plane is 11.7.5.2's mechanism; RC-08.2's test lays a translucent object, a masked one, a grouped one and an image with a soft mask over transferred ones |
| An object's shape for the plane is not the shape it painted | Each paint writes from the coverage it composited through, without the soft mask; an image's shape is drawn by the same sampler; a tile is held to the page |
| Black generation reaches colours 11.7.5.3 keeps it from | The colour's family decides; a `g` and a `k` fill under both functions are pinned at their own conversions |
| Exact knockout moves committed renders | RC-08.6 measures the knockout fixtures' partly covered pixels first and the owner decides; text knockout is taken only where it differs for whole pixels, so opaque text keeps its bytes |
| A screen or a type 1 cell differs across targets | Integer thresholds and comparisons; the cell's sine and cosine from `tinker-pdf-math`, built once; RC-08.11 fingerprints each halftone type on four targets |
| A hostile halftone or a page of distinct parameter sets | Threshold arrays decoded within `MAX_DECODED_STREAM` where they screen; a type 1 cell capped at 2²⁰ pixels; the plane's table capped at 255 sets, each refusal named |
