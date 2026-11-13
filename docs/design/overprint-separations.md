# Overprint and separations

A page renders as the inks a press would put down — cyan, magenta, yellow,
black and every spot colour its resources name — and from them either as one
composite picture with overprint simulated (roadmap row RC-03) or as one plate
per colorant (RC-04). Both stand on one ink model: which colorants a page
uses, how a colour's per-colorant tints reach the device beside the light it
already carries, the canvas that composites them, and how a composite of
plates becomes the light or ink a caller asked for. This document sets that
model out with RC-03's overprint; *Separations* below adds RC-04's plates.

## Scope

- **The ink model.** The colorants a page uses: the four process colorants
  of a CMYK device, `Cyan`, `Magenta`, `Yellow` and `Black`, and each spot
  colorant a `/Separation` or `/DeviceN` space on the page names (8.6.6.4,
  8.6.6.5). A colour carries its tints per colorant beside its light, as a
  DeviceCMYK colour already carries its four inks (`GraphicsState::fill_ink`
  in `crates/tinker-pdf-content/src/state.rs`); a CMYK-plus-spot canvas
  composites them, one channel per colorant; and a closed form turns that
  composite into the requested output.
- **Overprint simulation** (RC-03): `/OP`, `/op` and `/OPM` (8.6.7), stored by
  K-01 in `StoredParameters` as `stroke_overprint`, `fill_overprint` and
  `overprint_mode`, applied on that canvas through 11.7.4's compatible
  overprint blend, per colour-space kind and per kind of paint — fill,
  stroke, glyph, stencil mask, uncoloured pattern, image, inline image,
  shading and mesh — and under constant alpha, blend modes, soft masks and
  isolated, non-isolated and knockout groups.
- **A switch, off by default**, `RenderOptions::simulate_overprint`, and
  `tpdf render --overprint` to set it. Off, no page that renders today
  changes and none pays for any of this.
- **The leaf's channel mask**: `tinker-pdf-raster` composites a source into a
  chosen set of a canvas's channels, knowing channels and not colorants
  (ruling 8).

## Non-goals

- **Trapping** (FO-24), **flattening** transparency into opaque objects
  (FO-10), **halftones and transfer functions** (RC-08), **soft proofing**
  through an output profile (RC-05), and **PDF/X's overprint rules** (SD-11)
  are rows of their own. The composite here is the engine's own relation, not
  an output intent's.
- **SVG output** (`to_svg`) draws light and states no inks; nothing here
  reaches it.
- **10.8.3's simulation through flat XYZ.** 10.8.3 says a simulation of
  separations *should* match one that converts each separation to flat XYZ
  and multiplies them; the composite here multiplies in the engine's sRGB,
  where every other colour this engine draws lives (*The composite*, below).
- **Overprint inside a soft mask's group.** 11.7.3 substitutes alternate
  spaces for spot colours in a soft mask's group, and the group is read as a
  luminosity or an alpha; it renders as it does today.

## Design

### The colorants a paint names

Every paint names a set of colorants and a tint for each, decided by the
colour space it is painted in. 11.7.4.3's note 2 says which space that is:
the current colour space for a fill, stroke, glyph or stencil mask, the
image's own `/ColorSpace` for an image, the shading's for a shading; an
`/Indexed` space's base; and a `/Separation` or `/DeviceN` space's alternate
where the space reverts to it. Each space is of one **kind**:

| Kind | Spaces | Colorants it names |
| --- | --- | --- |
| Direct CMYK | DeviceCMYK as the current colour of a fill, stroke, glyph, stencil mask or uncoloured pattern — `k`, `K`, or `cs`/`scn` in DeviceCMYK — and such a colour under `/DefaultCMYK` | the four process colorants, at the colour's own components |
| Process | every other device, CIE-based or ICC space; DeviceCMYK in an image, inline image or shading | the four process colorants, at the ink its light turns into |
| Colorants | `/Separation` and `/DeviceN` painting their colorants directly | each named colorant, process or spot, at its tint |
| All | `/Separation /All` | every colorant of the canvas, at the one tint |
| None | `/Separation /None`, and `/DeviceN` whose names are all `/None` | nothing: the paint has no effect on the page (8.6.6.4) |

**A DeviceCMYK colour under `/DefaultCMYK` is direct CMYK.** 8.6.5.6's
note says a remapped colour's space is the default's, and that provisions for
device spaces then do not apply to it; 8.6.5.7 says that where a processor
uses a CIE-based colour as the device's own, "all of the semantics of the
device colour space shall also apply", the non-zero overprint mode first among
them, and that this need not depend on whether the space arrived through a
default. H-135's answer is that an ink render keeps a `k` colour's own
components under `/DefaultCMYK`, and `resolve_ink` (`FontSource`, answered in
`crates/tinker-pdf/src/resources.rs`) already asks the space as selected, not
as remapped. So on the CMYK-plus-spot canvas, which is an ink render, such a
colour paints its own components on the process channels — 8.6.5.7's
implicit conversion — and 8.6.7's overprint mode reads them as DeviceCMYK's.
An `ICCBased` CMYK colour set directly keeps today's light, so it is a
process colour, turned to ink as every other light is.

**Light turns into ink** by 8.6.4.4's relation inverted with maximum
undercolour removal (`rgb_to_cmyk` in `crates/tinker-pdf-raster/src/canvas.rs`),
as a `CmykA8` page does today.

**A `/Separation` or `/DeviceN` paints its colorants directly** when every
name it gives, `/None` aside, is a colorant of the canvas: a process name
(8.6.6.4 reserves `Cyan`, `Magenta`, `Yellow` and `Black` for the process
colorants of a CMYK device) or a spot the canvas carries. Otherwise it
reverts to its alternate space, as on a light page, and is a process colour.
A `/DeviceN`'s `/None` components are discarded when it paints directly, and
handed to its tint transform when it reverts (8.6.6.5). An `NChannel` space
is read component by component, as 8.6.6.5 asks: *Separations* settles its
`/Process` and `/Colorants`.

### A colour carries its colorants

`GraphicsState` keeps, for the fill and for the stroke, the colour's kind and
its tints by colorant beside its light — the generalisation of `fill_ink` and
`stroke_ink`, which carry the four inks of a DeviceCMYK colour and nothing
else. The interpreter sets them where it resolves a colour (`k`/`K`,
`cs`/`CS`, `sc`/`scn` and their uppercase twins), from a `FontSource` call
that generalises `resolve_ink`: it answers a colour's colorant tints by name —
a `/Separation`'s one, a `/DeviceN`'s each, DeviceCMYK's four — and, for a
spot the render composites, its alternate's process values. `cs` resets them
to the space's initial tints, `rg` and `g` carry none (their kind is process),
and `Q` restores them. **A light render pays nothing for it**: the call is
made only for a device that composites colorants, the state holds one pointer
per colour, `None` on every other device, and `fill_ink` keeps its meaning for
the `CmykA8` page. *Separations* gives the fields and the call their names.

### The canvas

The overprint simulation draws the page on RC-04's CMYK-plus-spot canvas: one
byte per colorant — cyan, magenta, yellow, black, then each spot the page's
inventory names — and an alpha, at the canvas's origin in the page frame
(ruling 5). A channel holds a **tint**, a quantity of ink: subtractive, as
`CmykA8`'s channels are, so 11.3.5's blend functions, written for additive
values, see each channel complemented on the way in and the way out, and
11.7.4.5 says the special overprint blend is complemented the same way. The
canvas starts with no ink, opaque, or transparent with
`RenderOptions::transparent`.

The page is drawn there when it has no transparency group, or one whose `/CS`
is DeviceCMYK. A page or group whose `/CS` is another space composites its
contents in that space, as today, with every spot through its alternate —
11.7.3's second option for a spot in a group, which its first sentence allows
— and is composited onto its parent as light turned into ink. So inside such
a group there are no process inks and no spots to overprint, which is what
11.7.4.5's note 1 says of a group whose space is not the device's.

### Overprint: which channels a paint marks

Table 146 (11.7.4.5) gives the blend function `B(Cb, Cs)` of the special
overprint mode per kind and per affected component, `Cs` meaning the paint's
tint replaces the backdrop's and `Cb` that the backdrop's stays. A paint
**marks** the channels whose `B` is `Cs`, and leaves the rest:

| Kind | `/OP` (stroke) or `/op` (fill) false | true, `/OPM` 0 | true, `/OPM` 1 |
| --- | --- | --- | --- |
| Direct CMYK | every channel: the four process tints, every spot at 0 | the four process channels | each process channel whose component is not zero |
| Process | every channel: the four process tints, every spot at 0 | the four process channels | the four process channels |
| Colorants | every channel: the named tints, every other channel at 0 | the named channels | the named channels |
| All | every channel at the tint | every channel at the tint | every channel at the tint |
| None | no channel | no channel | no channel |

**The zero test is the file's tint.** 8.6.7: whether a component is zero is
decided on the value in the file, before it is quantised — so `0.001 k`'s
cyan is not zero though it is the byte 0, and its channel is marked, at zero.
`/OPM 1` is the non-zero mode and every other value is mode 0: K-01 stores the
integer written and 8.6.7 defines two.

**Which flag.** A fill, a glyph drawn by filling, a stencil mask, an image, an
inline image and `sh` read `/op` (`fill_overprint`, which is `/OP` where `/op`
is absent); a stroke and a glyph drawn by stroking read `/OP`
(`stroke_overprint`). A shading pattern and an uncoloured tiling pattern read
the flag of the operation they fill or stroke.

**Per paint:**

- A **fill, stroke or glyph** marks by its colour's kind; `/OPM 1` applies,
  since each uses the current colour (8.6.7).
- A **stencil mask** paints the current colour through an image's samples and
  marks as a fill of that colour does, `/OPM 1` included: it is the current
  colour, not a sampled one, that DeviceCMYK's row of Table 146 excludes.
- An **uncoloured tiling pattern** (`/PaintType 2`) paints its cell in the
  colour its `scn` gave, in the pattern's underlying space, and marks by that
  space's kind.
- An **image** or **inline image** marks by its own space's kind, DeviceCMYK
  being a process space here: 8.6.7 says the non-zero mode shall not apply to
  images. A `/DeviceN` image marks its named channels at every sample.
- **`sh`, a shading pattern and a mesh** mark by the shading's space's kind,
  with the same exception for DeviceCMYK (8.6.7).
- A **coloured tiling pattern** (`/PaintType 1`) draws its cell in the
  canvas's format and composites it as a group.
- A **group** composites every channel with `B = Cs` whatever the state's
  flags: Table 146's last row, because nothing records which channels its
  contents marked.
- **`B`, `B*`, `b`, `b*` and text modes 2 and 6** fill and stroke as one
  object (11.7.4.4): with `/OP` and `/op` both true and the two alphas equal,
  inside a non-isolated, non-knockout group in which the fill and the stroke
  are painted at alpha 1 with the overprint blend, the group then composited
  with the original alpha and blend mode; otherwise inside a non-isolated
  knockout group in which each is painted with its own alpha, the prevailing
  blend mode and its own overprint flag, the group then composited at alpha 1
  under `Normal`.

### Overprint under transparency

The special mode's `B` sits inside 11.3's compositing formula like any other
blend function, so a channel a paint does not mark is
`Cr = (1 − αs/αr)·Cb + (αs/αr)·((1 − αb)·Cs + αb·Cb)`, with `Cs` that
channel's tint in the paint — zero, for a channel the paint names no tint
for (11.7.3: an unspecified component is a subtractive tint of 0). Over an
opaque backdrop that is `Cb`: the channel is untouched. Over a backdrop that is
not opaque the erasing is reduced in proportion, as 11.7.4.2 says it is. A
marked channel takes the blend mode in force, as every channel does today.

- **Constant alpha and soft masks** enter as `αs` and change nothing about
  which channels are marked.
- **A blend mode other than `Normal`.** 11.7.4.3: the overprinting object is
  painted with the special mode into an implicit non-isolated, non-knockout
  group, and the group's result is composited with the current blend mode.
  The object is painted at its own alpha inside the group and the group
  composited at alpha 1, so the alpha is applied once. Read literally, a
  channel the object does not mark comes out of the group as its backdrop,
  `Cg = Cb`, and the group composite then applies the mode to it:
  `B(Cb, Cb)`, which is `Cb` for `Darken` and `Lighten` and is not for
  `Multiply` — a cyan backdrop under a magenta overprinting `Multiply` fill
  darkens where the fill covers it. That is what the clause's construction
  computes, and it is what this design does.
- **An isolated group** starts from a transparent backdrop, so its first
  paints see `αb = 0` and take `Cs` in every channel: overprint inside it acts
  only on what the group itself painted, and the group then composites every
  channel onto its parent.
- **A non-isolated group** sees its parent's channels as its initial backdrop
  (11.4.4), so an overprinting paint inside it leaves them as it would outside,
  and 11.4.7.2's removal and the group's composite reproduce the paint drawn
  without the group: the twin RC-03.7 holds.
- **A knockout group** composites each object over the group's initial
  backdrop (11.4.5): `knock_out` restores every channel under the object's
  shape before it is drawn, so `Cb` for a channel the object leaves is the
  initial backdrop's, not the earlier object's.

**Spot channels and blend modes.** 11.7.4.2: only separable, white-preserving
modes apply to spot colorants, and `Difference`, `Exclusion` and the four
non-separable modes apply to the process channels alone while spots take
`Normal`. The canvas knows which of its channels are spots (the four process
channels come first), so the leaf substitutes `Normal` there itself.

### The composite

The simulated canvas is converted to the requested format per pixel, from
nothing but that pixel's channels, so a tile converts as the page under it
does (ruling 5). For a pixel with process tints `C, M, Y, K` and spot tints
`t₁ … tₙ`, as bytes:

- **Light** (`Gray8`, `GrayA8`, `Rgb8`, `Rgba8`): `cmyk_to_rgb(C, M, Y, K)` —
  8.6.4.4's relation in integers, as a `CmykA8` buffer reads back — multiplied
  channel by channel (`mul255`) by `Sᵢ(tᵢ)` for each spot, where `Sᵢ(0)` is
  white and `Sᵢ(t)` is the sRGB the spot's alternate space gives its tint
  transform's output at `t/255`: the colour of that colorant alone. Multiplying
  is 10.8.3's step c, done in sRGB.
- **Ink** (`CmykA8` with `RenderOptions::allow_cmyk`): the process tints, and
  each spot's ink alone folded into them channel by channel as
  `x ← 255 − mul255(255 − x, 255 − s)` — the same multiply, in additive terms.
  A spot's ink alone is its tint transform's output where its alternate is
  DeviceCMYK, and `rgb_to_cmyk(Sᵢ(t))` where it is not.
- **Alpha** is the canvas's.

A spot's **colour alone** is its `/Separation` space's alternate — the first
such space the inventory meets for the name — or, for a spot only a
`/DeviceN` names, that space's `/Colorants` entry for it (8.6.6.5: a
`/Separation` there describes the colorant alone), or, with neither, the
`/DeviceN`'s tint transform at the spot's tint with every other component at
zero. This is the model RC-03.3's test recomputes from the plates, in closed
form.

### The leaf's channel mask

`tinker-pdf-raster` gains `Channels`, the set of a canvas's colour channels a
source marks: a bit per channel, channel `i` being bit `i`, wide enough for
the four process channels and every spot plate the cap admits. `Channels::ALL`
marks every channel. A caller puts a set in force on a canvas
(`Canvas::set_marked`), as it holds ink tables in force, and every entry that
composites a source reads it — `Canvas::fill_mask_inked`,
`Canvas::fill_mask_plates`, `Canvas::blend_pixel_inked` and the level forms
images and runs draw through, and `Canvas::composite` with its tinted and
inked forms — so the light entries that wrap them (`fill_mask`,
`fill_mask_with`, `blend_pixel`, `blend_pixel_with`) composite under the set
in force, `Channels::ALL` on every canvas no caller has put one on. In
`blend`, a channel outside the set takes `B = Cb`
inside 11.3.6's formula, written above; a channel in it takes the mode's `B`
as today; with `Channels::ALL` every line is today's, so every format's bytes
are unchanged. A non-separable mode over ink is computed through light, as
today (`approximated_blends`), and a channel outside the set then takes
`Cb` — a combination the renderer never asks for, since 11.7.4.3 puts a
non-`Normal` mode outside the object, in its implicit group, but one the leaf
answers rather than leaves undefined. Masks apply to `CmykA8` and to the
CMYK-plus-spot format; on the light formats a partial set is accepted and
composites as the same rule over their channels. The leaf knows channels and
never colorants, overprint flags or `/OPM`: the renderer turns a colour's kind,
its tints and the state's flags into a set, by the table above, through the
render's plate plan (*Separations*).

### The switch

`RenderOptions::simulate_overprint`, `false` by default. On, `Page::render`
and `DisplayList::render` draw the page on the CMYK-plus-spot canvas — chosen
where the page-group and ink formats are chosen in `Page::render_layer_with`
(`crates/tinker-pdf/src/lib.rs`) — and convert it by *The composite* to the
format asked for. Off, nothing in this document runs. A display list records
the colours' kinds and tints with the switch on, as `CmykA8` already records
`fill_ink`, and counts them in its byte bound (`kept_bytes`). `tpdf render
--overprint` sets it and changes no other default. A page whose plates would
pass RC-04's cap renders as RC-04's warning says, the spots past it composite
through their alternates.

## Milestones

The steps of RC-03 and their exit tests. RC-03 depends on K-01, which stores
the keys, and on RC-04, whose canvas and colorants it draws on.

1. **RC-03.2 — channel masks in the leaf.** `canvas.rs`:
   `a_source_paints_only_its_channels` — for each separable blend mode, a
   source alpha of 1 and 0.5 and a backdrop alpha of 1 and 0.5, each marked
   channel equals 11.3's composite and each unmarked one
   `(1 − αs/αr)·Cb + (αs/αr)·((1 − αb)·Cs + αb·Cb)`, both computed in the
   test, on `CmykA8` and the CMYK-plus-spot format;
   `a_full_channel_mask_composites_as_before` — `Channels::ALL` gives every
   format's bytes as before, over the existing compositing tests' inputs.
2. **RC-03.3 — the switch and the composite.** `overprint.rs` (new):
   `the_switch_converts_the_composite_by_the_model` — a page painting two
   spots and process inks without overprint renders, switched on, the sRGB
   *The composite* computes from its plates, as `Rgb8` and as `CmykA8` with
   `allow_cmyk`, and switched off byte-identical to a render without the
   switch; `render_regions.rs`'s `an_overprint_simulated_page_tiles_as_the_page`
   holds every tile of it, switched on, byte-equal to the page.
3. **RC-03.4 — `tpdf render --overprint`.** `tools/tpdf/src/main.rs`:
   `render_overprint_sets_the_switch_and_nothing_else` — the parsed options
   equal the default with the switch on, and the usage text names the flag.
4. **RC-03.5 — flat colours overprint.** `overprint.rs`:
   `each_overprint_case_paints_its_closed_form` — over `/OP` and `/op` on and
   off, `/OPM` 0 and 1, and each kind of the first table, fills, strokes,
   glyphs and stencil masks over a backdrop with ink in every channel, each
   pixel equal to the composite the test computes from the second table and
   11.3's formula; the twins: a DeviceCMYK colour under `/OPM 1` and a
   `/Separation` of the same single process colorant mark the same channel;
   `/op` absent and `/op` written equal to `/OP` render byte-identical; a
   `k` colour under `/DefaultCMYK` and the same colour without the default
   render byte-identical with the switch on; `0.001 k`'s cyan is marked under
   `/OPM 1`; a `/Separation /None` paint leaves every channel as it was.
   Both builds.
5. **RC-03.6 — sampled sources.** `overprint.rs`:
   `images_and_shadings_overprint_by_their_space` — an image XObject, an
   inline image, an axial shading and a mesh in DeviceCMYK under `/OPM` 0 and
   1 (marking all four process channels under both), in `/Separation` and
   `/DeviceN`, and in an RGB space, each over a backdrop in every channel,
   equal to the composite the test computes; the twin: a one-sample image
   paints as a flat fill of its colour under the same state, `/OPM 0`.
6. **RC-03.7 — under transparency.** `overprint.rs`:
   `overprint_under_transparency_composites_as_designed` — an overprinting
   fill at alpha 0.5, under `/Multiply` (through the implicit group), under a
   soft mask, and inside isolated, non-isolated and knockout groups, and a
   `B` and a mode-2 glyph with both flags true at equal and at unequal alphas,
   each over a backdrop in every channel and each pixel equal to the composite
   the test computes from *Overprint under transparency*; the twins: a
   non-isolated group's contents against the same contents drawn outside it,
   and an overprinting `Darken` fill against the same fill under `Normal`
   where every channel it leaves is opaque.
7. **RC-03.8 — fingerprints.** `determinism.rs`'s fixture table gains a
   render-options field, and pages overprinting a fill, a glyph, an image, a
   shading and a group over process and spot channels join
   `rendering_is_stable_across_targets` with the switch on, and
   `a_display_list_replays_every_fingerprinted_page_byte_for_byte` replays
   them; a fixture with default options hashes as before.
8. **RC-03.9a and RC-03.9b — the speed row.** `render an overprinted
   CMYK-plus-spot page at 150 dpi`, one of RC-03.8's pages, in MS-04's render
   group, landed unmeasured and timed in the end phase.

There is no RC-03.10: reading the print-production visual suites' pages —
overprint, transparency and spot-colour test pages made to be judged by eye —
is the owner's (H-81), and `tpdf render --overprint` is the tool for it.

## Risks

| Risk | Mitigation |
| --- | --- |
| The simulation is taken for what a press prints. It is 11.7.4's compatible blend and a multiply of plates in sRGB, not an output intent's characterisation of real inks | The composite is a closed form stated here and recomputed by RC-03.3's test; proofing against an output profile is RC-05's, and the visual suites' pages are read by a person (H-81) |
| The literal reading of 11.7.4.3's implicit group darkens a channel the object does not mark under a mode that is not idempotent (`Multiply` and its kind) | Transcribed as the clause constructs it and pinned by RC-03.7's test, so a different reading is a visible change; the visual suites are where a person sees it |
| The switch slows or bloats every render | Off by default; with it off no colorant is asked for, no state field is set and no canvas changes; RC-03.3's off-switch render is byte-identical |
| A `k` colour under `/DefaultCMYK` overprints differently from what 8.6.5.6's note reads | 8.6.5.7 and H-135 decide it, stated above; RC-03.5's twin pins it |
| A page names more spots than the canvas can carry | RC-04's cap and its warning: the spots past it composite through their alternates, named |
| A non-separable mode on ink approximates through light | As today on `CmykA8`, counted and reported (`RenderWarning::ApproximatedGroupBlend`); spots take `Normal` there (11.7.4.2) and are not approximated |

## Separations

RC-04: a page rendered to its plates — one image of ink per colorant — on the
canvas the overprint simulation draws on, and the inventory of spot colorants
a page names.

### Scope

- **The page's spot-colorant inventory**: a facade query listing, once each,
  every spot colorant the page's resource scopes name.
- **The CMYK-plus-spot format** of `Canvas` in `tinker-pdf-raster`: four
  process channels and a channel per spot beside an alpha, the format RC-03
  composites overprint on.
- **A plate plan**: the process plates and the inventory's spots, each drawn
  as its own plate, composited through its alternate onto the process plates,
  or disabled.
- **Every paint reaching plates**: flat colours, glyphs, stencil masks,
  images of any component count, shadings, meshes, coloured and uncoloured
  patterns, groups and soft masks.
- **Plates handed out** as one-channel images of ink with their colorant's
  name and kind, for the separations writers (FO-03, FO-04).
- **At least 64 spot plates**, under a byte bound a page's plates may not
  pass.

### Non-goals

- **Writing separations files**: FO-03 and FO-04 write them, from what this
  hands them.
- **Halftoning a plate**, transfer functions, black generation and
  undercolour removal (RC-08), and **trapping** (FO-24).
- **A `/DeviceN`'s `/MixingHints`** — `/Solidities`, `/PrintingOrder`,
  `/DotGain`. 8.6.6.5 says a processor need not use them, and a composite
  through the tint transform is what every other path here does.
- **An output intent's colorants** (14.11.5) as the process set. The process
  plates are always the four of a CMYK device.
- **Overprint**: plates are drawn with knockout, every channel a paint does
  not name erased, as 8.6.7 does with overprint off; RC-03 applies `/OP`,
  `/op` and `/OPM` on these plates.

### Design

**The format.** `PixelFormat::Plates { spots }` — cyan, magenta, yellow,
black, then `spots` spot channels in the plan's order, then an alpha,
interleaved per pixel: `5 + spots` bytes. Every channel is a tint,
subtractive, complemented through 11.3.5's blend functions as `CmykA8`'s are,
so four process channels and no spots composite byte for byte as `CmykA8`
does — RC-04.3's twin. The five formats there are today keep the five-byte
compositing word they composite through (`encode`, `source_from`, `blend`),
untouched: a plate paint builds a word of its own width once per paint, a
slice of `4 + spots` tints, and composites through it, so no light or ink
render runs a line it did not run before. A canvas is allocated for the
spots the page's inventory names, never for the cap. `Channels`, RC-03's
mask, is a set wide enough for every channel the cap admits.

**The plate plan.** The four process plates and the inventory's spots, in
that order, each with a behaviour:

- **Spot**: its own plate. Every colorant's default.
- **Composite**: no plate of its own; its paints reach the process plates
  through its alternate space (*Colours on plates*). Asked of a process
  colorant, it is read as Spot: the process plates are what a composite lands
  on.
- **Disabled**: the colorant keeps its plate while the page is drawn — so it
  knocks out, and is knocked out by, every other paint exactly as a Spot plate
  is — and the plate is left out of what is handed back, and out of RC-03's
  composite.

The overprint simulation draws with every spot a Spot plate.

**Names on plates** (8.6.6.4, 8.6.6.5):

- `Cyan`, `Magenta`, `Yellow` and `Black` are reserved for the process
  colorants of a CMYK device: in a `/Separation` or a `/DeviceN` they name the
  process plates, never a spot plate of the same name, and the inventory leaves
  them out.
- **`/All`** applies its tint to every colorant available on the device, the
  process colorants included (8.6.6.4): every plate the canvas carries, Spot
  or Disabled. A Composite spot has no plate and is not a colorant the device
  has, so `/All` gives it nothing. `/All` is a `/Separation` name only —
  8.6.6.5 forbids it in a `/DeviceN` — and a `/DeviceN` naming it reverts to
  its alternate. Inside a group whose `/CS` is not DeviceCMYK, where the
  canvas's plates are not the group's colorants (11.7.3's note 2), it is drawn
  as the group's light draws it.
- **`/None`** marks nothing (8.6.6.4); a `/DeviceN`'s `/None` components are
  discarded when it paints its colorants directly, and a `/DeviceN` whose
  names are all `/None` never paints and never reverts (8.6.6.5).
- **Available.** A `/Separation` or a plain `/DeviceN` paints its colorants
  directly when every name it gives, `/None` aside, is available — a process
  name, or a spot whose plan behaviour is Spot or Disabled — and otherwise
  reverts to its alternate as a whole. An `NChannel` space is read component
  by component (8.6.6.5): each component the device has paints its plate, and
  each it has not — a Composite spot — reaches the process plates through its
  own alternate, its `/Colorants` entry.
- **An `NChannel`'s `/Process`.** Components of a CMYK process space — the
  four reserved names, or the names `/Components` gives a DeviceCMYK or a
  four-component CMYK `ICCBased` process space — paint the process plates. The
  components of any other process space are converted together through that
  space, with the components the names array leaves out at the space's initial
  values, to light, and the light to ink on the process plates. Inks an
  `NChannel` colour puts on one plate — a process component, a Composite
  spot's own ink, the other process space's — add, held at solid ink.

**Blend modes on spot plates** (11.7.3, 11.7.4.2): a spot colorant blends
under a separable, white-preserving mode only — every separable mode but
`Difference` and `Exclusion`. Under those two and the four non-separable
modes the mode applies to the process plates alone, the non-separable ones
through light as on `CmykA8` (`approximated_blends`), and every spot plate
takes `Normal`. The format knows which of its channels are spots, so the
substitution is the leaf's.

**Colours on plates.** `ColorSpace::Separation` keeps the colorant names its
`/Separation` or `/DeviceN` array gives — bytes, `/All` and `/None` told apart
from a colorant named `All` — and a `/DeviceN`'s `/Subtype`, `/Colorants` and
`/Process`, and `parse_space` fills them as `describe_space` already reads
them for `Page::images`. A colour carries its colorants (*A colour carries its
colorants*, above): `GraphicsState` gains `fill_colorants` and
`stroke_colorants`, an `Option<Arc<Colorants>>` each — the space's kind, each
named colorant's tint as a byte, and, for a spot the plan composites, the
alternate's process values — from `FontSource::colorants`, which generalises
`resolve_ink` and is asked only by a device that composites plates. The
renderer maps them by the plan: a Spot or Disabled colorant to its channel, a
Composite one through its alternate's process values onto the process
channels, a process name to its process channel, `/All` to every channel and
`/None` to none; a colour of process kind, which carries no colorants, is its
light turned into ink (`rgb_to_cmyk`). A DeviceCMYK colour under
`/DefaultCMYK` keeps its own components there, as *The colorants a paint
names* settles. A recorded event counts its colorants in `kept_bytes`, so
`MAX_DISPLAY_LIST_BYTES` still bounds what a recording holds.

**Images on plates.** An image or inline image in DeviceCMYK, `/Separation`,
`/DeviceN` or an `/Indexed` space over one of them keeps each sample's
colorant tints through decode beside `DecodedImage`'s light — for a plates
render alone, so a light render decodes as it does today — and a `/DeviceN`
image's distinct colours are converted once each through the 4 096-slot table
its light already goes through. `draw_image`'s policy — the unit square's
coverage through `fill`, `Area` and `Bilinear` sampling by ratio, the pyramid
past 128:1 — samples N components onto the plates format with integer weights
(ruling 4), each plate as the one-component image of its colorant alone would
be sampled, and a run of abutting plate images is accumulated per plate as a
light run is per colour, so no seam opens between them. An image in any other
space reaches the process plates as its light turned into ink. A stencil mask
paints the fill colour's colorants.

**Shadings on plates.** An axial, radial or function-based shading, painted by
`sh` or as a shading pattern, in DeviceCMYK, `/Separation` or `/DeviceN`,
evaluates its function to colorant tints per pixel — from the same evaluation
`Shading::color_at` converts to light, before `to_rgb`, so the light path keeps
its bits. A mesh (types 4 to 7) interpolates each colorant tint per pixel in a
mesh buffer of N components, with the one seamless fill and the fringe spread
exactly as for light. Any other space is light turned into ink.

**Patterns on plates.** A coloured tiling pattern's cell (`/PaintType 1`) is
drawn in the page's plates format and tiled channel for channel from the
page-frame lattice (ruling 5), as `CmykA8` tiles today. An uncoloured one
(`/PaintType 2`) paints the colorants of the colour its `scn` gave.

**Groups on plates.** A group whose `/CS` is DeviceCMYK, or absent, composites
in a plates buffer of its own, carrying the page's spots: isolated, or
non-isolated — adopting its parent's channels as its backdrop and removing
them at the close, as `adopt_backdrop` and `remove_backdrop` do for ink
(11.4.4, 11.4.7.2) — and knockout (11.4.5). A spot painted inside it keeps its
plate (11.7.3's first option: "the spot colour passes directly through the
group hierarchy"). A group whose `/CS` is another space composites in that
space with every spot through its alternate (11.7.3's second option), and its
buffer composites onto the plates as light turned into ink, every spot channel
under its shape at no ink: a group paints every component (Table 146's last
row). The group budget, `MAX_GROUP_BUFFERS`, counts a plates buffer as one
group; its bytes are charged with the page's plates against the byte bound
below, and a group that would pass it composites as a group of another space
does, its spots through their alternates, under the cap's warning.

**Soft masks on plates.** A soft mask's group renders in its own space — in
`CmykA8`, a buffer of ink, where the group names none — and is read as a
luminosity or an alpha, as on an ink page: 11.7.3 substitutes the alternate
space for every spot in it, so a colour there paints its light, and the mask
scales every plate alike.

**The inventory.** `Page::spot_colorants()` lists each spot colorant once, as
a `SpotColorant` carrying its name, in the order the page's scopes first name
it: the scope walk `crates/tinker-pdf/src/fontlist.rs` makes (`walk`, and
`annotation_resources` for the normal appearances), a `/Resources`
dictionary's entries in the order it holds them — its `/ColorSpace` entries,
the spaces of its images, shadings, patterns and groups, an `/Indexed` space's
base, a `/Pattern` space's underlying space, each form's, tiling pattern's,
Type 3 font's and soft-mask group's own resources where the walk enters them,
then each annotation's appearance — and within a `/DeviceN` its names in
array order, then its `/Colorants` keys. It reads spaces with `describe_space`
(`crates/tinker-pdf/src/resources.rs`), which already keeps the names for
`Page::images`, rather than a fourth reader. Process names, `/All` and `/None`
are left out. A scope already walked is not walked again, so a resource cycle
ends; the walk's own bounds end a walk past them with what was read (ruling
1); and listing records nothing in `Document::warnings`.

**Plates handed out.** `Page::separations(&SeparationOptions)` renders through
`render_layer_with`'s one pipeline (ruling 5) — the region, the scale, the
rotation, antialiasing and every warning as for `Page::render` — on the plates
format, and hands back `Separations`: each plate the plan does not disable, the
process plates first and then the spots in inventory order, as a `Plate` with
its colorant's name, its kind (process or spot) and its tints, one byte a
pixel, 0 no ink and 255 solid. A plate is not a `Bitmap`: a one-channel bitmap
reads as grey, light, and a consumer that read a plate as one would invert
it. With `RenderOptions::transparent` the page's alpha comes back beside the
plates. `SeparationOptions` carries the `RenderOptions` and each colorant's
behaviour by name, every one Spot by default.

**The cap, and what it costs.** `MAX_SPOT_PLATES` is 64: 68 colour channels
and an alpha, 69 bytes a pixel. At `MAX_PAGE_PIXELS` (2^26, in
`crates/tinker-pdf-render/src/lib.rs`) those are 69 × 2^26 bytes, 4.3 GiB,
which no render may ask for, so the plates are bounded in bytes as well:
`MAX_PLATE_BYTES` is 2^30, 1 GiB, every plates buffer live at once counted —
the page's and each open group's. A spot page at A4 and 300 dpi, 2 480 × 3 508
pixels, keeps all 64 spot plates in 600 MB; a page at `MAX_PAGE_PIXELS` keeps
16 bytes a pixel, the four process plates, the alpha and eleven spots. Past
either cap the spots past it in inventory order are drawn as Composite, and the
render carries `RenderWarning::PlatesCapped { named, kept }` (ruling 10); the
overprint simulation does the same. Both caps join `bounds_ledger.rs`.
`MAX_SPOT_PLATES`'s comic and book columns are zero — a comic archive is
images, and a book's CSS colours are sRGB, so neither synthesises a
`/Separation` or a `/DeviceN` — and its fixed-document column is 28: an XPS
`ContextColor` of 2 and of 5 to 8 channels is synthesised as a `/DeviceN`
named after its channel count (`crates/tinker-pdf/src/xps/profiles.rs`), and
2 + 5 + 6 + 7 + 8 names is the most one page can hold. `MAX_PLATE_BYTES`'s
columns are those plates at each format's page size at 150 dpi, the dpi
`MAX_PAGE_PIXELS`'s row is at: 5 bytes a pixel for the comic's
4 167 × 6 250 and the book's 900 × 1 350, and 33 for the fixed document's
1 275 × 1 650.

**What a light render pays.** Nothing per paint: no colorant is asked for, and
the two state fields are `None`, a word each, which a `q` copies. The light and
ink formats composite through their own paths, byte for byte, and every
fingerprint stands. `PixelFormat` gains a variant, so a `match` over it gains
an arm; `page_format` hands a caller asking for it `Rgba8`, as it does
`CmykA8` without `allow_cmyk` and `LabA8`.

**The plates' sum.** A page drawn with every spot Composite paints each spot's
alternate onto the process plates, and the same page drawn to plates paints it
on its own plate and erases the process plates there. Folding each spot plate
back through its tint transform and adding it to the process plates gives the
composite: byte for byte where every pixel is wholly covered (antialiasing
off), every paint is opaque and `Normal`, every tint the file states is a whole
level (`k/255`), and each spot's tint transform is linear from no ink into
DeviceCMYK — a type 2 function with `N` 1, `C0` all zero and each `C1`
component 0 or 1 — because then the plate's byte and the composite's are one
rounding of one number; and within one level of 255 per channel otherwise,
where a partly covered pixel is two roundings on plates and one in the
composite. RC-04.10's test holds that equality.

**The seam RC-03 takes.** RC-03 composites on this format through `Channels`:
the renderer turns a colour's kind, its colorants and the state's flags into
the set of channels a paint marks, by RC-03's table and this plan, and the
leaf composites the rest as the backdrop.

### Milestones

1. **RC-04.2 — the inventory.** `separations.rs` (new):
   `the_colorant_inventory_is_every_spot_its_resources_name` — over fixtures
   putting spots in each scope and each space kind, the inventory equals the
   set the test reads from the fixture's own resource dictionaries, each once,
   in the walk's order; a spot only an unreachable object names is absent; a
   resource cycle and a walk past its bound end with what was read; listing
   adds nothing to `Document::warnings`; the `render_page` fuzz target calls
   it.
2. **RC-04.3 — the format.** `canvas.rs`:
   `four_process_channels_composite_as_cmyka8_does` (the same masks, inks,
   alphas and separable modes give C, M, Y, K and alpha byte-identical to a
   `CmykA8` canvas), `a_paint_knocks_out_the_channels_it_does_not_name`,
   `sixty_eight_channels_each_keep_their_own_ink` and
   `a_canvas_past_the_channel_cap_is_refused_by_name`; every existing
   `canvas.rs` test unchanged.
3. **RC-04.4 — transparency on plates.** `canvas.rs`:
   `spot_groups_composite_as_cmyk_groups_do` (isolated, non-isolated and
   knockout groups of process ink give `CmykA8`'s bytes, backdrop removal
   included), `a_spot_under_a_non_isolated_group_comes_back_as_it_went_in`,
   and `non_separable_modes_reach_spot_channels_as_transcribed` (`Normal` on
   every spot channel under `Difference`, `Exclusion` and the non-separable
   modes, the mode on the process channels).
4. **RC-04.5 — names in the colour model.** `tinker-pdf-color`'s
   `a_separation_keeps_its_colorant_names` and `resources.rs`'s
   `parse_space_and_describe_space_read_the_same_names` (a `/Separation`, a
   `/DeviceN` with attributes, `/All`, `/None`, a colorant named `All` inside a
   `/DeviceN`, and an `/Indexed` over a `/DeviceN`); `colour_spaces.rs`,
   `writer_tints.rs` and every fingerprint unchanged.
5. **RC-04.6 — a colour carries its colorants.** `interpret.rs`:
   `a_colour_carries_the_colorants_its_space_answers` (each operator path
   records them; `cs` resets them to the space's initial tints; `rg` and `g`
   carry none; `Q` restores them; a device that composites no plates asks for
   none).
6. **RC-04.7 — the resources answer.** `resources.rs`:
   `the_seam_answers_each_spaces_colorants` (DeviceCMYK, a `/Separation`, a
   `/DeviceN` naming a process colorant, `/All`, `/None`, an `NChannel` with a
   CMYK and an RGB `/Process`, an `/Indexed` over a `/DeviceN` and a `k`
   colour under `/DefaultCMYK`); `display.rs`:
   `a_recorded_state_counts_its_colorants`.
7. **RC-04.8 — flat colours on plates.** `tinker-pdf-render`:
   `a_spot_paint_lands_on_its_plate_and_knocks_out_the_rest` (fills, strokes,
   a glyph and a stencil in a `/Separation`, a `/DeviceN` and DeviceCMYK put
   exactly their tints on their plates and none on the others) and
   `process_plates_draw_as_the_ink_canvas_does` (a page in DeviceCMYK,
   DeviceGray and DeviceRGB draws C, M, Y and K byte-identical to its
   `CmykA8` render).
8. **RC-04.9 — groups and coloured patterns.** `tinker-pdf-render`:
   `a_group_on_plates_composites_as_a_cmyk_group_does`,
   `a_spot_inside_a_group_keeps_its_plate` (and through an RGB group, its
   alternate on the process plates), `a_coloured_cell_tiles_its_plates`.
9. **RC-04.10 — separations on the facade.** `separations.rs`:
   `n_spots_render_n_plus_four_plates_whose_sums_reproduce_the_composite` —
   for 1, 2 and 8 spots in `/Separation` and `/DeviceN`, DeviceCMYK
   alternates, beside process fills and text, the plates' sum equals the page
   drawn with every spot Composite by *The plates' sum*;
   `each_plate_behaviour_paints_as_it_says` (Spot, Composite, Disabled, and
   Composite asked of a process colorant); `render_regions.rs`'s
   `separation_tiles_are_the_page_s_plates`; both builds.
10. **RC-04.11 — images of any component count.** `image.rs`:
    `each_plate_of_an_image_samples_as_its_one_component_image` (1:1, 2:1 and
    3.7:1 down, 4× up with and without interpolation, past 128:1) and
    `a_run_of_abutting_plate_images_leaves_no_seam`.
11. **RC-04.12 — an image's samples on plates.** `separations.rs`:
    `an_image_in_each_ink_space_lands_on_its_plates` (stored and inline, at
    1:1 and 2:1, the tints from the samples, `/Decode` and the palette).
12. **RC-04.13 — shadings on plates.** `separations.rs`:
    `a_shading_in_each_ink_space_lands_on_its_plates` (types 1, 2 and 3, the
    tint the shading's own function gives at each pixel centre).
13. **RC-04.14 — meshes on plates.** `mesh.rs`:
    `an_n_component_mesh_interpolates_each_component_as_light_does`;
    `separations.rs`: `a_mesh_in_each_ink_space_lands_on_its_plates`.
14. **RC-04.15 — the cap, shown.** `separations.rs`:
    `sixty_four_spots_render_sixty_eight_plates` and
    `a_page_past_the_plate_cap_reports_what_it_left` (past `MAX_SPOT_PLATES`,
    and past `MAX_PLATE_BYTES` at a page the byte bound reaches first: the
    spots kept, the rest Composite, `RenderWarning::PlatesCapped` naming both
    counts); `bounds_ledger.rs` holds a row for each cap with the columns
    above.
15. **RC-04.16 — fingerprints.** `separations.rs`:
    `separations_are_stable_across_targets` — the SHA-256 of every plate of a
    fixture with spots in fills, text, a group, an image, a shading and a mesh,
    its text in an embedded face so one hash serves both builds, entered in the
    determinism roster.
16. **RC-04.17a and RC-04.17b — the speed rows.** The separations fixture's
    plates at 150 dpi and the 64-spot page's 68 plates at 72 dpi in MS-04's
    render group, landed unmeasured and timed in the end phase; no target is
    set for them here.

### Risks

| Risk | Mitigation |
| --- | --- |
| A plate render's memory: 69 bytes a pixel at 64 spots is 17 times an `Rgba8` page | Planes only for the spots the page names; `MAX_PLATE_BYTES` over every live plates buffer, the page's and its groups'; past it, Composite spots and a named warning, in the ledger with a test that fires it |
| A light or ink render moves | The five formats keep their compositing word and paths; colorants are asked for only by a plates device; RC-04.3 and RC-04.8 hold `CmykA8` to its bytes and every fingerprint stands |
| `PartialEq` on `ColorSpace` starts telling two spaces apart by a colorant's name | RC-04.5 reads every comparison of spaces in the tree before it lands |
| A spot named twice with two different alternates | One plate per name, which is what a name means (8.6.6.4); its colour alone is the first definition the inventory meets |
| Plates are read as light | A `Plate` is tints with a name and a kind, not a `Bitmap`; the composite of plates is RC-03's closed form, not a reading of plate bytes as grey |
| A spot inside a group of another space loses its plate | 11.7.3's second option, stated and tested (RC-04.9); a group of DeviceCMYK, or of none, keeps it |
