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
marks every channel. Every entry that composites a source takes one —
`Canvas::fill_mask_inked`, `Canvas::blend_pixel_inked` and the level form
images draw through, and `Canvas::composite` — and the light entries that
wrap them (`fill_mask`, `fill_mask_with`, `blend_pixel`, `blend_pixel_with`)
pass `Channels::ALL`. In `blend`, a channel outside the set takes `B = Cb`
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
