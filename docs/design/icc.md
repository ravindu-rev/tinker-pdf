# ICC colour management

The engine has its own CMM — profile parsing and transform evaluation in the
`tinker-pdf-color` leaf, no third-party colour engine — and honours
transparency-group colour spaces in compositing (ISO 32000-1 11.4.7, 11.6.6),
the page-level `/Group` included. ICC profiles drive conversion, held to
known-answer tables computed from the specification's own equations
([render-verification](render-verification.md)). A profile the parser refuses
falls back to `ColorSpace::Approximated`, honoured by component count; what is
open of this design — the refused profiles — is listed in the
[roadmap](../ROADMAP.md) on its own.

## Scope

- **Group colour spaces.** `/Group /CS` on form XObjects (11.6.6) and on the
  page object (11.4.7), resolved through the `parse_space` seam in
  `crates/tinker-pdf/src/resources.rs`, and honoured: blending happens in the
  declared space, conversion happens once at group boundaries, never per
  element.
- **An own CMM**, the `icc` module of `tinker-pdf-color`: profile header and
  tag table parsing (ICC.1:2010, ISO 15076-1; v2 profiles included, since PDFs
  embed them constantly), TRC curves (`curv`, `para`), matrix/TRC transforms
  (`rXYZ`/`gXYZ`/`bXYZ`, `wtpt`), and LUT transforms (lut8 `mft1`, lut16
  `mft2`, v4's `mAB `) read from a profile's `A2B*` tags, compiled into a
  profile→PCS→destination pipeline. `chad` is not read (see
  [Three decisions](#three-decisions-and-why)).
- **Rendering intents** (8.6.5.8): the `ri` operator, an ExtGState's `/RI`
  and an image's `/Intent` select which of a profile's transforms converts a
  colour, by ICC.1's precedence of tags, and absolute colorimetric scales by
  the profile's media white
  ([Rendering intents](#rendering-intents-8658)).
- **Determinism under ruling 4** ([rulings.md](../rulings.md)): transform
  *evaluation* is integer and fixed-point only; anything transcendental
  happens once at table-construction time via `tinker-pdf-math`.
- **Typed refusals under rulings 2 and 10**: a profile the parser declines
  falls back to `Approximated` with a typed `IccError` naming the reason,
  never a hard failure.
- Fixtures, fingerprints, a fuzz target and specification-derived
  known-answer tests for all of the above
  ([verification.md](../verification.md), ruling 13).
- **Default colour spaces** (8.6.5.6; roadmap row K-02): a scope's
  `/DefaultGray`, `/DefaultRGB` and `/DefaultCMYK` stand in for the device
  space each is named for, on every painting path that selects one
  ([Default colour spaces](#default-colour-spaces-8656) below). A default is a
  CIE or `ICCBased` space like any other, and the CMM converts it.

## Non-goals

- **Output intents and proofing** (14.11.5): `/OutputIntents` handling
  belongs to [pdfa](pdfa.md); this design converts for the screen, to
  sRGB, always. Soft proofing is [ROADMAP](../ROADMAP.md) row RC-05.
- **N-channel output, overprint and spot-ink simulation.** `Separation` /
  `DeviceN` keep flowing through their tint transforms (8.6.6.4, 8.6.6.5);
  simulating ink interaction is a different renderer. Overprint simulation and
  separations are [ROADMAP](../ROADMAP.md) rows RC-03 and RC-04, designed in
  [overprint-separations.md](overprint-separations.md).
- **Black point compensation, gamut mapping beyond intent selection,
  iccMAX/v5 profiles, named-colour tags.** Refused by name, per ruling 2;
  [ROADMAP](../ROADMAP.md) rows RC-02 and RC-20.
- **Profiles embedded inside image codecs.** JPX colour boxes have their own
  path in `tinker-pdf-filters`; routing them through this CMM is scheduled by
  corpus evidence (ruling 3), not assumed here.
- **A bare `/CalGray` or `/CalRGB` name.** `parse_space` reads both arrays'
  white point, gamma and matrix (8.6.5.2–8.6.5.3); a bare name, which carries
  no parameters, is drawn as its device space.
- **For default colour spaces, the scope a Type 3 glyph or a soft-mask group
  runs in.** Both run in the scope that invokes them rather than in their own
  `/Resources` (see [the scope a default is read in](#the-scope-a-default-is-read-in)).
  A default follows the scope the engine resolves their other names in; the
  scope is the gap, and it is not this design's.
- **For default colour spaces, PDF/A's and PDF/X's reading.** The validators
  record a default per walk rather than per resource dictionary
  (`crates/tinker-pdf/src/pdfa/colour.rs`), a simplification they name; this
  design changes rendering and extraction, not validation.

## Design

### Stage order: group spaces before the CMM

The group space changes which numbers the blend formulas ever see: a
`Multiply` between two colours inside a CMYK group is a different formula
from the same blend in RGB, whatever profile fidelity each colour had. The
CMM changes the exact shade of managed colours. Stage 2 slots into the seam
stage 1 builds: an `ICCBased` group `/CS` is keyed by its `N` like any other
space the group machinery handles.

### What the corpus carries

`crates/tinker-pdf/tests/icc_census.rs` reads the header and tag table of every
ICC profile in the corpora — bytes only, sharing no code with the colour
crate, so it cannot agree with a parser that is wrong — and counts them by
kind (matrix/TRC, grey, LUT), device class and version;
`what_the_parser_makes_of_the_corpus_profiles` then hands them to the parser.
A file that carries a profile stream is not a file that paints through an
`ICCBased` space: a PDF/A file's `/OutputIntent` declares the space it was
prepared for whether or not any content is painted through it.

### Stage 1: transparency-group colour spaces

**Plumbing.** `tinker_pdf_content::Group` carries `space: Option<GroupSpace>`,
the shape of the group's `/CS`, which the resource seam resolves for a form
XObject's `/Group` (`form_from`, reduced to a shape by `group_space`). The
page dictionary's `/Group` (7.7.3.3) is read by `page_group_space` in
`Page::render` and decides the format of the page canvas itself, which is
converted back for the caller at the end — 11.4.7's page group composited,
then converted to the output device's space. A form's group composites in a
buffer of its own through `open_group`/`close_group` in
`crates/tinker-pdf-render/src/lib.rs`, under `MAX_GROUP_DEPTH` and the
`MAX_GROUP_BUFFERS` budget.

**Blending in the declared space.** The group buffer holds components of
the group's space plus alpha. `PixelFormat` in
`crates/tinker-pdf-raster/src/canvas.rs` has `CmykA8` and `LabA8` beside
`Gray8`/`GrayA8`/`Rgb8`/`Rgba8`; group spaces are keyed by shape
(`GroupSpace`: one channel, three, four, or Lab, mapped to a buffer by
`group_format`), which covers `/DeviceGray`, `/DeviceRGB`, `/DeviceCMYK`,
`ICCBased` by `N`, and `/Lab`, whose components are encoded into 0..1 before
11.3.5's formulas can say anything about them. Separable blend modes (11.3.5)
run per channel in `crates/tinker-pdf-raster/src/blend.rs`; the formulas
assume additive components, so CMYK channels enter and leave them
complemented. Non-separable modes (11.3.5.3) are defined on RGB values: on a
CMYK buffer the operands convert to RGB, blend, and convert back, an
approximation named by `RenderWarning::ApproximatedGroupBlend`.

**Conversion at boundaries only**, at exactly three points: painting a source
colour into a group whose space differs (source components → group space),
initialising a non-isolated group's backdrop (parent space → group space,
11.4.4), and `close_group` compositing the finished buffer onto the parent
(group space → parent space). The device relations of 8.6.4.4 supply the
forward direction and their inverse — RGB→CMYK with `k = min(1-r, 1-g, 1-b)`
undercolour removal (`rgb_to_cmyk`) — supplies the backward one.

**That inverse is exact, not an approximation.** Maximum undercolour removal
makes each channel's intermediate error smaller than half a level, so
RGB→CMYK→RGB is the identity for all sixteen million colours —
`the_cmyk_round_trip_is_exact` sweeps it as 32 896 `(v, max)` pairs, since
`K` is fixed by the maximum and each channel is then independent. What *is*
approximate is the other direction: a CMYK value that did not come from the
inverse — a rich black — comes back as its pure-K equivalent. Such values
exist: a DeviceCMYK colour's own components cross the resource seam beside
its light (`FontSource::resolve_ink`, `GraphicsState::fill_ink`), and a CMYK
buffer — a `/DeviceCMYK` group's, or a page asked for in ink — composites
them. Leaving a CMYK group for an additive parent takes them through 8.6.4.4
forward, which needs no inverse and is exact; a non-isolated group over a
parent of its own format adopts the backdrop as the parent's bytes and takes
the same bytes out at close (`Canvas::adopt_backdrop`, `remove_backdrop`);
what does not round-trip is a non-separable blend inside the group, whose
operands go to light and back, and `RenderWarning::ApproximatedGroupBlend`
names that. Soft-mask luminosity (11.6.5.2) reads the group's own space:
`to_mask` reads through `Canvas::pixel`, which applies the group's own
relation, and weights by 11.3.5.3's `Lum` — the clause's 0.3/0.59/0.11, not
`Color::luma`'s Rec.601 coefficients, the pair `blend.rs` records as "the
difference between matching a reference renderer and not".

**Proof.** In `crates/tinker-pdf/tests/transparency_groups.rs`, pairs that
differ only in the group's `/CS` are asserted *unequal*. `Difference` carries
the pair, not `Multiply`: writing the ink split as `k = 1 - max(r,g,b)` makes
the complemented components exactly `(R/max, G/max, B/max, max)`, so a
separable blend `f` recombines to `R' = f(R1/max1, R2/max2) * f(max1, max2)`.
For a product the two `max` terms cancel and the answer is `R1 * R2` — RGB's
answer — so a multiplied pair renders *identically* in both spaces and would
pass on a build that ignored `/CS` altogether. The invariance is kept as a
*second* test: an opaque `Multiply` must agree in both spaces within a level
(`an_opaque_multiply_is_the_same_in_either_space`), which catches a complement
applied on one side of the blend and not the other.

### Stage 2: the CMM

**The leaf boundary, argued.** Ruling 8 wants bytes and plain parameters
in, values out. Profile parsing fits exactly: `icc::Profile::parse(&[u8])`
sees ICC bytes, never a COS stream — the facade's `ICCBased` arm decodes the
stream and hands bytes across, as it hands function dictionaries across.
Evaluation lives in the same leaf, and the precedent is `Function::eval` in
`crates/tinker-pdf-color/src/function.rs`: the leaf owns both the parse
and the interpreter because the tables are meaningless without the exact
interpolation defined over them, because the fuzz target must drive
parse *and* eval to prove never-panic on hostile profiles, and because the
alternative — handing raw tables to `tinker-pdf-render` to interpret —
moves ICC vocabulary into the renderer and splits ruling 4's obligations
across two crates. The renderer calls `ColorSpace::to_rgb`.

**Compilation, then integer evaluation.** `Transform::compile(&Profile,
Intent)` compiles a profile's transform for one rendering intent and produces:
per-channel curves sampled to 4 096-entry
tables (`CURVE_ENTRIES`), a 3×3 matrix in s15.16 fixed point (ICC's own
`s15Fixed16Number`, so matrix-based profiles are *natively* fixed point) that
is the profile's columns already multiplied by XYZ-to-sRGB, and for LUT
profiles a grid interpolated over its axes in integers. Sampling a `para`
curve needs `pow` — that is `tinker_pdf_math::pow`, exactly as `lab_to_rgb`
uses it, because `tinker-pdf-color` is on the `PIXEL_PATHS` list that
`cargo xtask libm` enforces and a platform `powf` in a TRC is precisely the
cross-target divergence ruling 4 exists to stop. The PCS leg reuses the D50
XYZ→sRGB path; a LUT profile's connection space is read both as XYZ and as
v2's legacy 16-bit Lab encoding, which goes through the `finv` machinery
`lab_to_rgb` owns. A grey profile compiles its one curve to a luminance the
three sRGB channels share.

**Wiring.** `ColorSpace::Icc { transforms: Arc<Transforms>, components }` is
built by the `ICCBased` arm from the stream bytes and `/N`. `Transforms` holds
the profile and its transforms, one an intent: relative colorimetric's is
compiled with it, and each other intent's the first time
`ColorSpace::to_rgb_in` asks for it ([Rendering
intents](#rendering-intents-8658)). Any profile
the parser declines keeps the `Approximated` fallback, its refusal (bad
signature, unsupported tag, dimension over bounds) a typed `IccError`. Rows in
`crates/tinker-pdf/tests/bounds_ledger.rs` hold the maximum accepted profile
size and the maximum tag count. `MAX_CLUT_ENTRIES` refuses by name and is
reachable, but has no row: the yardstick that would give it one is a grid
volume, and the corpus census counts tags and bytes.

### Verification

- **Fuzzing:** the `icc_profile` cargo-fuzz target, seeded from
  `fuzz/corpus/icc_profile`.
- **Known-answer tables, not a second CMM.** Ruling 13 forbids asking
  another colour engine what a transform should produce, so the answers
  come from the specification instead. For a matrix/TRC profile every step
  is published arithmetic — the parametric curve types of ICC.1 6.2, the
  s15.16 matrix, the chromatic adaptation of Annex E — so a grid of inputs
  has expected outputs computable by hand. `icc.rs`'s own tests pin the
  fixed-point encodings bit pattern by bit pattern
  (`the_fixed_point_encodings_are_exactly_what_the_format_says`) and a linear
  curve's compiled ramp entry by entry across all 4 096
  (`a_linear_curve_compiles_to_a_linear_ramp`). This is *stronger* than a
  budgeted agreement with another CMM.

  **What it does not reach:** LUT profiles (`mft1`, `mft2`, `mAB `). Their
  grid interpolation is where two CMMs legitimately differ, and it is exactly
  the part no published table settles. Hand-computed lookups pin the
  interpolation this engine chose; nothing says that choice matches what a
  profile's author expected. That is a named limit of this design.
- **Corpus evidence:** the `tpdf probe` capability scan
  (`scan_capabilities` in `tools/tpdf/src/main.rs`) reports an `iccbased` tag
  beside `jbig2` and `jpx`, so `corpus/report.json` counts the real files that
  carry profiles.
- **Fingerprints:** committed fingerprints in `determinism.rs` hold ruling 4's
  bit-identical contract across targets, a page under `ICCBased` and CIE
  default spaces among them.

### The price of an integer grid

Conversion evaluates a transform's whole pipeline for every colour, as Stage
2 describes. What a grid standing in for that pipeline would cost is priced
by a probe in `icc.rs`'s tests, `the_grid_prices_the_design_quotes`, and
held in a committed table, `crates/tinker-pdf-color/data/icc_grid_prices.tsv`,
which the probe rewrites:

```bash
cargo test -p tinker-pdf-color --release -- --ignored the_grid_prices_the_design_quotes
```

- **What is priced.** One profile of each model the tree reads, built in the
  test as the module's other fixtures are: matrix/TRC at gamma 1.0 and 2.2,
  grey under XYZ and under Lab, a four-input `mft2` printer table into Lab
  and into XYZ, and a three-input `mAB ` carrying all five of its stages.
  For each, a 17- and a 33-point grid whose nodes hold
  `Transform::apply`'s sRGB bytes at the node.
- **How a grid is read.** In 16-bit fixed point, each channel rounded once at
  the end: linearly over one input, tetrahedrally over three, and over four
  in each form a four-input grid can take — the 4-simplex's five nodes,
  tetrahedra over C, M and Y at the two K nodes either side and linear
  between them, and all sixteen corners multilinearly.
- **Against what.** `apply` itself, the engine's own evaluation, which the
  known-answer tests above pin; no other colour engine is asked (ruling 13).
  The sweep is of 8-bit inputs — every value of one input, every fifth of
  three, every seventeenth of four — and each row records a grid's nodes, its
  bytes at three a node, the pipeline evaluations its build made, and its
  worst and mean difference from `apply` in levels of 255.
- **Why it cannot flatter itself.** Before a grid is priced, every one of its
  nodes is read back through every form and must be `apply`'s answer
  exactly, so a grid built or read wrongly fails rather than reporting a
  price.

Which grid the engine builds from these, and how it is read, is
[P-22's section](#p-22--colour-conversion-throughput) below.

### Rendering intents (8.6.5.8)

K-01 keeps the intent in the graphics state: `ri` and an ExtGState's `/RI`
set `StoredParameters::rendering_intent`
(`crates/tinker-pdf-content/src/state.rs`), `q` and `Q` save and restore it
with the rest of the state, and a name 8.6.5.8 does not list is kept as
`RenderingIntent::Unrecognised`. The colour crate converts in any intent:
`Profile::parse` keeps each `A2B*` table a profile carries and
`ColorSpace::to_rgb_in` converts through the one an `icc::Conversion`'s
intent selects, by the table below. No conversion the facade makes passes an
intent yet, so every ICC colour converts in relative colorimetric — for a LUT
profile through the first of `A2B1`, `A2B0` and `A2B2` it carries — and an
image's `/Intent` is not read. How often the corpus names an intent is not
measured: the count `Profile::parse`'s comment gives is of an ExtGState
`/RenderingIntent`, a key the graphics state parameter dictionary does not
have; its key is `/RI`. This section is the design RC-01's steps build, read
from ISO 32000-2 and from ICC.1:2010 (ISO 15076-1:2010), the edition Table 66
names for PDF 1.7 and 2.0.

#### What the two standards say

- **Where an intent is named** (8.6.5.8): by `ri` (Table 56), by an
  ExtGState's `/RI` (Table 57) and by an image dictionary's `/Intent`
  (Table 87), each a name. Table 69's four — `AbsoluteColorimetric`,
  `RelativeColorimetric`, `Saturation` and `Perceptual` — shall be
  recognised, and a processor that does not recognise a name uses
  `RelativeColorimetric`. The graphics state starts at
  `RelativeColorimetric` (Table 51).
- **Not on the profile's stream.** Table 65, the entries an `ICCBased`
  stream adds, holds `/N`, `/Alternate`, `/Range` and `/Metadata` and no
  intent. 8.6.5.5 has a reader ignore the intent a profile's header records,
  take the intent from PDF's own parameter, and use only a profile's `AToB`
  information for a source colour.
- **An image's own intent** (Table 87) is the one "used in rendering any
  image that is not an image mask": it is ignored where `/ImageMask` is
  true, and its default is the current intent of the graphics state. An
  inline image carries it under its full name (Table 91).
- **When it is read** (11.7.5.3): when the conversion is made. An elementary
  object converts in the intent in effect at its painting operation; a
  transparency group whose space is CIE-based, painted into a parent of
  another space, in the intent in effect at its `Do`.
- **Which tag serves an intent** (ICC.1 8.10.2), for an input, display,
  output or colour space profile: a) the `DToB` tag designated for the
  intent, where it is present and supported, a tag holding an element the
  CMM does not support counting as unsupported; b) else the `AToB` tag
  designated for it; c) else `AToB0`; d) else the tone curves and matrix
  columns, or `grayTRC`. Table 25 designates `AToB0` perceptual, `AToB1`
  colorimetric and `AToB2` saturation, and 9.1 has `AToB1` serve both
  colorimetric intents; `DToB0` to `DToB3` are perceptual, media-relative
  colorimetric, saturation and ICC-absolute colorimetric (6.2.3, 9.2). Table
  25 calls a TRC/matrix or `grayTRC` model colorimetric in an input or a
  display profile and leaves it undefined in an output one. 8.2 lets one
  tag's data serve every intent, and the media-relative tag serve
  ICC-absolute colorimetry.
- **ICC-absolute colorimetric** (6.2.3, 6.3.2.2): `DToB3` where it is
  present, the media white unused; otherwise generated from the
  media-relative transform, its PCS value scaled by the source's
  `mediaWhitePointTag` over the PCS white — `Xa = Xmw / Xi × Xr`, and alike
  for `Y` and `Z` (equations 4 to 6) — and brought to the destination by the
  inverse ratio of the destination's own (equations 1 to 3). The PCS white is
  D50, `(0.9642, 1.0000, 0.8249)` (6.3.4.3); a display profile's
  `mediaWhitePointTag` is the PCS white (9.2.34), and every profile but a
  DeviceLink carries one (8.2).
- **A grey profile's PCS value** (F.2) is its curve's output times the PCS
  white, in XYZ or in Lab.

#### The table each intent compiles

| Intent | 8.10.2 a), from RC-01.4 | b) | c) | then | Media white |
| --- | --- | --- | --- | --- | --- |
| `Perceptual` | `D2B0` | `A2B0` | — | `A2B1`, `A2B2` | — |
| `RelativeColorimetric` | `D2B1` | `A2B1` | `A2B0` | `A2B2` | — |
| `Saturation` | `D2B2` | `A2B2` | `A2B0` | `A2B1` | — |
| `AbsoluteColorimetric` | `D2B3` | `A2B1` | `A2B0` | `A2B2` | scaled, except through `D2B3` |

- **Read across, first present wins.** The last column of tags is this
  design's, not ICC.1's: a profile that carries none of the tags 8.10.2
  names for an intent, and no curves, is read through its other tables in
  the order `Profile::parse` reads them today, so no intent refuses a profile
  the engine renders today. `RelativeColorimetric`'s row is today's order
  exactly, which is why a page that sets no intent converts as it does today.
- **Absolute colorimetric is relative colorimetric's table, scaled**, where
  no `D2B3` is read: 6.2.3 generates it from the media-relative transform,
  and 8.10.2 b) for it is `AToB1`. `D2B1` is not in its row: 8.10.2 names
  only `DToB3` for the absolute intent.
- **A tag the engine cannot read is passed over, not refused**: a `D2B*` tag
  holding an element this build does not read is unsupported in 8.10.2 a)'s
  sense, and the intent falls to its `A2B*` tag. Before RC-01.4 every `D2B*`
  tag is passed over.
- **Tags at one offset are one table** (8.2): read once, compiled once, and
  shared by every intent that selects it.
- **Matrix/TRC and grey profiles** (8.10.2 d)) serve `Perceptual`,
  `RelativeColorimetric` and `Saturation` from one transform, whatever the
  profile's class, and `AbsoluteColorimetric` from that transform with the
  media white folded in.
- **A profile carrying a matrix or a `grayTRC` beside `A2B*` tables** is read
  by its matrix or its curve, for every intent, as `Profile::parse` reads it
  today. 8.10.2 puts its tables first (b) and c) before d)); taking that
  order would move every such profile on every page, whatever intent the page
  sets, so it is the owner's to take and this design does not.

#### Absolute colorimetric against the engine's output

The engine's destination is sRGB through `XYZ_D50_TO_SRGB`
(`crates/tinker-pdf-color/src/lib.rs`), which takes the PCS white to sRGB's
white: its media white is the PCS white, as 9.2.34 has a display's be, so
equations 1 to 3 are the identity on the destination's side and the whole of
the intent is the source's scaling. With `s = (Xmw / Xi, Ymw / Yi, Zmw / Zi)`:

- **Matrix/TRC**: the scaling is folded into the s15.16 matrix once —
  `XYZ_D50_TO_SRGB × diag(s) × columns` — so a pixel costs what a relative
  one does.
- **Grey**: F.2's PCS value, the curve's `Y` on the PCS white (under Lab,
  `L*` with `a* = b* = 0`, the same white), scaled, is `Y × wtpt`. A grey
  profile whose media white is not D50 draws its greys in the medium's tint
  under this intent, through `XYZ_D50_TO_SRGB`; under the others it stays
  achromatic, as today.
- **A table** (`mft1`, `mft2`, `mAB `, and a `D2B*` other than `D2B3`): the
  PCS value it gives — XYZ, or legacy 16-bit Lab taken to XYZ by the
  conversion `lab_to_rgb` already makes — scaled per channel, then to sRGB as
  the relative path does.
- **The PCS white is 6.3.4.3's**, taken as the s15Fixed16 numbers a
  profile's `XYZType` tags carry (`0x0000F6D6`, `0x00010000`, `0x0000D32D`),
  so a `wtpt` of D50 scales by exactly one and absolute colorimetric is
  relative colorimetric bit for bit.
- **A profile with no `wtpt`, or one with a component that is not positive**,
  is taken to have the PCS white for its media white: 8.2 requires the tag,
  an `ICCBased` space's profile cannot be a DeviceLink (Table 67's classes),
  and taking the PCS white adds no colour the profile does not state. Its
  absolute colorimetric is its relative colorimetric.

#### Where each conversion takes its intent

| Conversion | Intent |
| --- | --- |
| A flat colour in an ICC space or under an ICC default — fill, stroke, glyph, stencil mask, an uncoloured pattern's colour | the state's, at the painting operator (11.7.5.3) |
| An image XObject or inline image, not an image mask | its `/Intent` where that is a name, `RelativeColorimetric` for a name 8.6.5.8 does not list; the state's at `Do` or `EI` where the entry is absent or not a name |
| A shading — `sh`, a shading pattern's fill, a mesh's vertices | the state's, at the painting operator |
| An `Indexed` base, a `Separation` or `DeviceN` alternate, a `[/Pattern base]` base, each in an ICC space | the intent of the conversion it serves |
| A transparency group's space at its boundaries | none: groups convert by device relations (Stage 1), and no profile runs there for 11.7.5.3's `Do` intent to select |
| A luminosity soft mask's `/BC` | `RelativeColorimetric`: the backdrop is neither an elementary object nor a group, which is all 11.7.5.3 names |
| `CalGray`, `CalRGB`, `Lab` | none: each is a closed form with no table per intent, and its white point is the diffuse white its components are relative to (8.6.5.2 to 8.6.5.4), adapted to the output's white, so its absolute colorimetric is its relative |
| A device space with no default | none: 8.6.5.8 converts CIE-based colours |

- **A colour set before the intent changed paints in the intent at the
  paint** (11.7.5.3). The interpreter resolves a colour to light when it is
  set (`sc`, `scn`, `g`, …), so it keeps beside the fill and the stroke
  colour the space each was set in and its components, and an intent that
  changes — by `ri` or by a `gs` with `/RI` — re-resolves both through
  `FontSource::resolve_color` in the new intent. `Q` restores a colour with
  the intent it was resolved in; setting the intent in force again resolves
  nothing.
- **An unrecognised name is `RelativeColorimetric`** wherever it is read: the
  state keeps `RenderingIntent::Unrecognised` (K-01), and the conversion
  reads it as `RelativeColorimetric`. `ri` with an operand that is not a
  name, and an `/RI` that is not one, change nothing (K-01).
- **An image's `/Intent` overrides the state's for that image only**, and an
  image mask's is never read: its samples are a stencil painted in the fill
  colour, which converts in the state's intent like any flat colour.

#### The one conversion parameter

`tinker_pdf_color::icc::Intent` is ICC.1's four (6.1), its default
`RelativeColorimetric`, and `tinker_pdf_color::icc::Conversion` carries it:
the one value every site that converts an ICC colour takes,
`ColorSpace::to_rgb_in(components, conversion)` among them, while `to_rgb`
converts in `Conversion::default()`. A space that is not ICC-based reads it
only to pass it to the ICC space it holds — an `Indexed` base, a
`Separation` alternate, a pattern's base. The facade maps the content
crate's `RenderingIntent` to `icc::Intent`, `Unrecognised` to
`RelativeColorimetric`, so the leaf keeps no PDF name for an intent. RC-02
adds black-point compensation to `Conversion` as a second field, and keys
what this design keys by intent by the whole value.

#### What compiling per intent costs

- **Parsing.** `Profile::parse` reads the tag table as it does today, and
  the table the default intent selects; it keeps a copy of each other `A2B*`
  element's bytes and reads one when an intent first selects it. A page that
  sets no intent parses what it parses today.
- **Compiling.** A space's transforms are kept together, each compiled the
  first time an intent selects its table, and shared by every conversion
  through that space and every page whose resources compile the profile. A
  profile used in one intent compiles one transform, as today. A LUT profile
  used in all four compiles at most three — one a distinct table — with
  absolute colorimetric's scaling applied after the relative table's
  evaluation; a matrix/TRC or grey profile at most two, the second only for
  absolute colorimetric with a media white other than D50.
- **Memory.** A LUT transform holds its grid, at most `MAX_CLUT_ENTRIES`
  16-bit entries, so a profile used in three intents holds at most three
  times what it holds today; a matrix/TRC transform is three 4 096-entry
  curves and a matrix, and its absolute twin shares the curves.
- **Caches key on the intent's table.** The facade's ICC cache keeps a
  profile's transforms beside each other, so an intent changing finds the
  profile already parsed; the decoded-image cache keys an image on the
  intent it converted in, since one image drawn under two intents is two
  conversions (RC-01.6).

#### The steps and their exit tests

- **RC-01.2**: `Transform::compile` takes an `icc::Intent` and
  `Profile::parse` keeps each `A2B*` table by the table above.
  `each_intent_compiles_its_own_table` (a profile carrying three different
  `mft2` grids answers each intent with its own, recomputed in the test by
  multilinear interpolation), `an_absent_intent_table_falls_back_as_icc_1_says`
  (each row of the table above with the tags its intent names absent),
  `tags_sharing_an_offset_are_read_once`, `write_the_fuzz_seeds` asserting
  the new `lut-three-intents.icc` seed reaches a transform, and the
  `icc_profile` fuzz target compiling every intent.
- **RC-01.3**: absolute colorimetric.
  `absolute_colorimetric_scales_by_the_media_white`: for a matrix/TRC, a grey
  and an `mft2` profile, under XYZ and Lab connection spaces, each with a
  `wtpt` other than D50, the absolute transform's sRGB equals the relative
  one's PCS value scaled in closed form and converted in the test; a `wtpt`
  of D50 makes the two intents equal byte for byte; a profile with no `wtpt`
  converts as above.
- **RC-01.4**: the `D2B*` tags, by the a) column of the table above.
  `a_d2b_tag_converts_by_its_element_pipeline`,
  `an_unread_element_is_refused_by_name` (and its intent falls to its
  `A2B*` tag), `an_impossible_element_count_is_refused_before_it_is_allocated`,
  and the `d2b-elements.icc` seed in `write_the_fuzz_seeds`.
- **RC-01.5**: flat colours. `rendering_intents.rs`'s
  `each_intent_paints_its_recomputed_transform` (a fill, a stroke and a
  glyph in `ICCBased` spaces over a LUT profile with three distinct tables
  and a matrix/TRC profile, each with a non-D50 `wtpt`),
  `ri_and_extgstate_ri_are_twins` (through `Q` and an intent changed after
  `scn`), and an unrecognised name painting `RelativeColorimetric`, in both
  builds.
- **RC-01.6**: images. `an_images_intent_selects_its_transform` (an image
  XObject and an inline image, per `/Intent`),
  `an_image_intent_and_ri_are_twins` (rendered and written to SVG), and one
  image drawn under two intents decoding once per intent.
- **RC-01.7**: shadings. `each_intent_shades_its_recomputed_transform` (an
  axial, a function-based and a mesh shading), the twin of a constant-colour
  axial shading's interior and the flat fill in the same intent, and a page
  painting a fill, an image and a shading under each intent among
  `determinism.rs`'s fingerprints.
- **RC-01.8**: the speed row `render an ICC page under each rendering intent
  at 150 dpi`, RC-01.7's page under all four intents, in MS-04's set.

### Default colour spaces (8.6.5.6)

Roadmap row K-02. A resource dictionary's `/ColorSpace` may carry
`/DefaultGray`, `/DefaultRGB` and `/DefaultCMYK` (PDF 1.1), and 8.6.5.6 makes
each stand in for the device space it is named for, so that device colours
are drawn calibrated. Rendering substitutes them on every painting path that
reads a space (`crates/tinker-pdf/tests/default_spaces.rs`), and
`crates/tinker-pdf/tests/default_space_census.rs` counts them by scope; the
PDF/A and PDF/X validators read them on their own walk
(`crates/tinker-pdf/src/pdfa/colour.rs`, `crates/tinker-pdf/src/pdfx/print.rs`).
Extraction reports the substitute beside the space an image states (below),
and the census's first figures are roadmap row K-02.5. This section transcribes what the standard
says, maps every selection it remaps and every scope a default is read in,
and settles what the K-02 rows build.

**Quoted from ISO 32000-1:2008, with no copy of it in the tree.** Where the
standard is silent, or this section is not sure what it says, it says so, and
each such place carries the decision this design takes and the test that
holds it, so a different reading moves one test.

#### What the standard says

8.6.5.6, in its own words:

> A colour space is selected for painting each graphics object. This is
> either the current colour space parameter in the graphics state or a
> colour space given as an entry in an image XObject, inline image, or
> shading dictionary. Regardless of how the colour space is specified, it
> shall be subject to remapping as described here.
>
> When a device colour space is selected, the ColorSpace subdictionary of
> the current resource dictionary is checked for the presence of an entry
> designating a corresponding default colour space (DefaultGray,
> DefaultRGB, or DefaultCMYK, corresponding to DeviceGray, DeviceRGB, or
> DeviceCMYK, respectively). If such an entry is present, its value shall
> be used as the colour space for the operation currently being performed.

The rest of the clause, in short:

- **Values pass unchanged.** The device space's colour values go to the
  default as they are; the default shall have the device space's number of
  components, and a value outside its range is adjusted to the nearest valid
  one. That it be compatible — the components' ranges, additive or
  subtractive — is a *should*.
- **Any space but Lab, Indexed or Pattern** may be a default.
- **A special space over a device space** has the default used in place of
  the underlying space: the underlying space of an uncoloured tiling
  pattern, the base of an Indexed space, and the alternate of a Separation
  or DeviceN space, "but only if the alternate colour space is actually
  selected".

Two other clauses reach it:

- **8.6.8.** `G` sets the stroking colour space "to DeviceGray (or the
  DefaultGray colour space; see 8.6.5.6, "Default Colour Spaces")", and `g`,
  `RG`/`rg` and `K`/`k` say the same of theirs, so the device operators are
  selections exactly as `cs` is. `CS`'s own entry adds that the names
  DeviceGray, DeviceRGB, DeviceCMYK and Pattern "always identify the
  corresponding colour spaces directly; they never refer to resources in the
  ColorSpace subdictionary": a `cs` operand naming one is never looked up,
  and the default is what reaches the resources.
- **11.6.6**, the group attributes dictionary's `/CS`: "Device colour spaces
  shall be subject to remapping according to the DefaultGray, DefaultRGB,
  and DefaultCMYK entries in the ColorSpace subdictionary of the current
  resource dictionary (see 8.6.5.6, "Default Colour Spaces")". So a group's
  device `/CS` remaps, and with it a luminosity soft mask's `/BC`, which
  11.6.5.2 counts in "the colour space specified by the CS entry in the
  group attributes dictionary". The same entry uses "default" for something
  else: an absent `/CS` defaults to the parent group's or page's space, the
  space a group inherits to blend in, which is not 8.6.5.6's default.

#### Which selections remap

| Selection | Remapped | Said by |
| --- | --- | --- |
| `g`/`G`, `rg`/`RG`, `k`/`K` | yes | 8.6.8 |
| `cs`/`CS` naming a device space, or a `/ColorSpace` entry that is one | yes | 8.6.5.6 |
| `sc`/`scn` in such a space, or with no space named | yes | 8.6.5.6 |
| An `/Indexed` base | yes | 8.6.5.6 |
| `[/Pattern base]`'s base | yes, in the scope whose `cs` selected it | 8.6.5.6 |
| A `/Separation` or `/DeviceN` alternate | when selected — which, as this section reads 8.6.6.4, `/All` and `/None` never are; the engine selects it for every other colorant, since no render paints one on a plate of its own | 8.6.5.6 |
| An image XObject's `/ColorSpace` | yes, a JPEG's and a JPEG 2000 image's among them | 8.6.5.6 |
| An inline image's `/CS` | yes, after `expand_inline_abbreviations`, an inline JPEG's and JPEG 2000 image's among them | 8.6.5.6, 8.9.7 |
| A shading's `/ColorSpace`, under `sh` and in a shading pattern, and its `/Background` with it | yes | 8.6.5.6 |
| A transparency group's `/CS`, a form's and the page's | yes | 11.6.6 |
| A luminosity soft mask's `/BC` | yes, through `/G`'s `/CS` | 11.6.5.2, 11.6.6 |
| A stencil mask, and an image's `/Mask` | nothing to remap | 8.9.6.2, 8.9.6.3 |
| A soft-mask image (`/SMask`) | no | read here from 11.6.5.3 |
| A JPEG 2000 codestream's own colour, under no `/ColorSpace` | no | read here |
| A space inside a default | not stated; no, here | — |
| A `/DeviceN`'s `/Process` space | not stated; not read here (RC-04.5) | 8.6.6.5 |

**Four of those answers are this design's readings, not quotations.** A
soft-mask image's samples are the parent image's opacity (11.6.5.3), and
8.6.5.6 remaps a space "selected for painting": nothing is painted in that
one, but this section cannot quote a sentence that says it is not remapped.
A JPEG 2000 codestream with no `/ColorSpace` over it carries its own colour
specification (7.4.9), which is not a PDF device space and is read as
selecting none. A default's own spaces are resolved as written and never
remapped again: nothing is said of a `/Separation` default whose alternate is
`/DeviceRGB` under a `/DefaultRGB`, and resolving the substitute once is the
reading that terminates — `/DefaultRGB /DeviceRGB`, a legal default, would
otherwise remap itself without end. Whether `/Process` remaps is RC-04.1's to
read when it reads the attributes. A stencil has no space of its own: it
paints the fill colour (8.9.6.2), remapped when that was set.

**The engine's own readings of a device space remap too.** `cs /G`, `/RGB`
or `/CMYK` where no resource has that name, and a bare `/CalGray` or
`/CalRGB` name with no parameters, are drawn as the device space in every
other respect (`color_space`), so a default applies to them as it does to the
names they stand for.

#### The scope a default is read in

**A default is read from the dictionary the stream's other names resolve
in.** 8.6.5.6 says "the current resource dictionary", and the engine keeps
one object per such dictionary, `PageResources`. Reading the default from
the object a `cs /CS0` beside it resolves against means the two are never
read in different scopes, including in the two rows below where the
engine's scope is itself the wrong one.

| Scope | The standard | This engine |
| --- | --- | --- |
| Page | its `/Resources`, inherited down the page tree (7.7.3.4) | `PageResources::new`, over the inherited dictionary (`crates/tinker-pdf-cos/src/pages.rs`) |
| Form XObject | its own `/Resources` (8.10.1); without them, the page's, a construct 7.8.3 calls obsolete | its own (`form_resources`), or the invoking scope when it has none |
| Tiling pattern | its own `/Resources`, required (Table 75) | its own, or the invoking scope's when absent |
| Type 3 glyph | the font's `/Resources`, or the page's when absent (9.6.5) | **the enclosing scope**: a glyph procedure runs without changing scope, which `docs/features/editing.md` records for subsetting |
| Annotation appearance | its own `/Resources`, an appearance stream being a form XObject (12.5.5) | its own, or an empty scope when it has none (`crates/tinker-pdf/src/annots.rs`) |
| Soft-mask group | `/G`'s own `/Resources`, `/G` being a form XObject (11.6.5.2) | **the scope that set the `gs`**, the deviation FT-28's notes record |

- **A page's default does not reach into a form that has `/Resources` of
  its own**, even resources with no `/ColorSpace`: they are the current
  resource dictionary inside it, and 8.10.1 makes an independent form's
  resources the whole of what it may name. A reader that carries a scope's
  defaults into the forms it invokes draws such a form in the substitute
  where this design draws it in the device space; the census
  (`default_space_census.rs`) counts the files where the two part. The other
  way, a form's default applies inside it only.
- **An uncoloured pattern's base is remapped where `[/Pattern base]` was
  selected**, since its colour is given there. A coloured cell paints under
  its own `/Resources`' defaults; an uncoloured cell paints no colour of its
  own (8.7.3.3).
- **An image or a shading brings no resources**, so it is read in the scope
  that paints it: `Do`, `sh`, or the fill a shading pattern paints.
- **A Type 3 glyph and a soft-mask group** run in a scope that is not the one
  9.6.5 and 11.6.5.2 name, for every name in them alike, and the default
  follows that scope rather than being put right alone (the non-goal
  above). A `d1` glyph specifies shape only (9.6.5), so only a `d0`
  glyph's colours are in question.

**A group's `/CS`: which "current resource dictionary", the standard does
not say.** For the page group it can only be the page's. For a form's group
it could be the form's own `/Resources`, where its contents are painted, or
the invoking stream's, where `Do` runs. This design remaps it by the defaults
of the scope the group's contents run in: the form's own when it has them,
the invoking scope when it has not, and for a soft-mask group the scope the
engine runs it in. A group whose `/CS` were remapped by one dictionary and its
contents by another would paint them in one calibration and composite them in
another. `form_from` reads a form group's `/CS` where `Do` runs and remaps it
by the defaults of the scope its contents run in, so what it tolerates is
reported where `Do` runs.

**Remapping a group's `/CS` changes no pixel, and the `/BC` is where a
default shows.** `group_space` keeps only a space's shape — one, three or four
channels, or Lab — and a default has its device space's component count, so a
remapped group composites in the same buffer through the same boundary
conversions; the remap would matter only to a group composited through a
profile. A luminosity mask's `/BC` is converted to light through the space, so
a default there moves the backdrop the mask is measured against.

#### One selection, two answers

**The space as selected and the space as substituted.** `resolve_color` and
every decode — images, inline images, shadings, the `/BC` — convert through
the substitute. `resolve_ink` and `initial_color` read the space as selected:
the first so that a DeviceCMYK colour keeps its ink under a default (below),
the second for the colour `cs` resets to. Both answers come from one parse of
the selection — the substitute is the parsed space with the scope's defaults
put in — so nothing is parsed twice.

**The initial colour is the device space's, carried into the default.** The
standard does not settle what `/DeviceCMYK cs` sets the colour to under a
`/DefaultCMYK`: DeviceCMYK's own `0 0 0 1` (8.6.8), passed unchanged into the
default as every other value is, or the default's own initial value, which
for an `ICCBased` space is all zeros — in CMYK, no ink at all. The two differ
wherever the default's own initial value is not the device space's: under an
`ICCBased` `/DefaultCMYK`, and under any default that is a `/Separation` or
`/DeviceN`, whose own initial value is the full tint. Under an `ICCBased`,
`CalGray` or `CalRGB` default for gray or RGB they agree: both are zeros, and
a range that leaves zero out moves both to the same nearest value. This
design carries the device space's value into the default, so
`/DeviceCMYK cs` and `0 0 0 1 k` paint one colour under one default, and
selecting a space never turns a page's black into paper. It follows that a
page selecting `/DeviceCMYK` and its twin naming the substitute directly
start from different colours, so the twins in `default_spaces.rs` set their
components before they paint.

**A default the clause does not allow is refused, not obeyed**: Lab,
Indexed or Pattern, a component count other than the device space's, or a
value that is no colour space. The device space is drawn as without it and
the refusal is named — `RenderWarning::RepairedColorSpace` under the
default's own name, its `reason` `DefaultSpaceFamily`,
`DefaultSpaceComponents` or `DefaultSpaceUnreadable`, once per render of the
scope (ruling 10). 8.6.5.6 says what may be a default and not what a reader
does with one that may not; drawing what the content asked for, and saying
so, is ruling 2's answer.

**The device operators set the space slot.** 8.6.8 has `g`, `rg` and `k`
select DeviceGray, DeviceRGB and DeviceCMYK, so the slot `sc` asks the default
by cannot go on naming whatever `cs` set last: `/CS1 cs 1 sc 0.5 g 0.7 sc`
paints `0.7` in DeviceGray. The operators set it to the reading by count,
which allocates nothing, and the reading by count asks the default of the
family it takes the components as: one is gray, four is CMYK, any other count
is RGB.

#### `k` under `/DefaultCMYK` on an ink render (H-135)

[H-135](../roadmap/H.md) is an open question: on an ink render
(`format: PixelFormat::CmykA8` with `RenderOptions::allow_cmyk`), does a
DeviceCMYK colour under a `/DefaultCMYK` keep the document's own components
as ink, or is it converted through the default like every other colour and
lose them? This section does not settle it. The engine builds H-135's
default:

- **The light render paints the default.** `k` under
  `/DefaultCMYK [/ICCBased …]` paints the profile's light, as `/CS0 cs … sc`
  naming that profile directly does.
- **An ink render keeps the document's ink**: `1 1 1 1 k` arrives as all four
  inks, from `fill_ink` (`crates/tinker-pdf-content/src/state.rs`). The light
  beside it, which an ink page reads wherever it needs light rather than ink —
  inside a group whose `/CS` is `/DeviceRGB`, say — is the default's.
- **The same for every flat DeviceCMYK colour**, which is everything that
  carries ink ([rendering.md](../features/rendering.md), "CMYK page
  output"): a fill, stroke or glyph colour set by `k` or `K`, by `sc`/`scn`
  in a space selected as DeviceCMYK, or by four components with no space
  named, and a stencil mask painted in one.
- **A light render composites in ink inside a `/DeviceCMYK` group.** The
  renderer composites a colour's ink wherever a canvas composites in ink: an
  ink page, and a `/DeviceCMYK` group's buffer on any render
  (`crates/tinker-pdf-render/src/lib.rs`). So inside such a group `k` under a
  `/DefaultCMYK` paints, on a light render, what the page without the default
  paints, not what its twin naming the profile paints, and the first bullet
  does not hold there. It follows from keeping the ink, and choosing
  conversion would change it with the rest; a guard row of
  `an_ink_render_keeps_a_device_cmyk_colour_s_ink_under_default_cmyk` holds
  it.
- **What carries no ink is not H-135's question.** An image, a shading, an
  uncoloured pattern's colour, and an `/Indexed` or separation space over
  DeviceCMYK reach an ink canvas as light taken back to ink; under a default
  that light is the default's, and there is no ink of the document's to
  keep.
- **The two contracts H-135 names part on such a page.** `Bitmap::to_png`
  writes an ink page as 8.6.4.4's light, and the light render paints the
  profile's, so a page painting `k` under `/DefaultCMYK` writes a PNG that is
  not its light render.
  `an_ink_page_written_as_png_is_the_light_the_switch_would_have_returned`
  (`crates/tinker-pdf/tests/render_options.rs`) holds two pages that carry no
  default and says nothing of this one. Choosing conversion instead is one
  change to `resolve_ink`, as H-135 says.

#### What `Page::images` reports for an image drawn under a default

`Page::images` reports the space an image states (`PageImage::color_space`,
`crates/tinker-pdf/src/images.rs`): a `/DeviceRGB` image is `DeviceRgb` under
any default. Beside it, it reports the space its samples are in:

- **`PageImage::substituted_space`** is the stated space with each device
  space in it that the scope remaps replaced by the default, described as
  `describe_space` describes any space: an image in
  `[/Indexed /DeviceRGB …]` under a `/DefaultRGB [/ICCBased …]` is
  `Indexed { base: Icc { … } }`. It is `None` where nothing was replaced, so
  a page with no default lists exactly what it lists without one.
  `PageImage::sample_space` answers the space the samples are in — the
  substituted one where there is one, the stated one otherwise — and is what
  `an_extracted_image_names_the_space_its_samples_are_in`'s twins compare.
- **One entry per image and space.** `Collector` lists an XObject once, by
  its reference, with every placement. Under defaults one XObject can be drawn
  in two spaces — on a page with a `/DefaultRGB` and in a form whose own
  `/Resources` have none — and is then listed once for each, each with its
  own placements. An inline image is its own entry.
- **A JPEG or JPEG 2000 image reports its substitute like any other.**
  Extraction describes the space the standard puts the samples in, which is
  the default's and the one the renderer draws them in.
- **A refused default replaces nothing**, and an image in its family names
  the refusal in `warnings`, as it names a decoder's leniency (ruling 10). A
  soft-mask image and a stencil have nothing substituted.
- **`to_svg` follows the render with no change of its own.** It writes the
  interpreter's `fill_color` (`crates/tinker-pdf/src/svg_out.rs`) and decodes
  images and shadings through the `GlyphSource` calls the renderer makes, so
  the substitution reaches it, which
  `a_default_space_page_writes_the_svg_its_twin_writes` holds.

#### Read once a scope, so a page with none pays nothing per operator

- **Where.** A scope's `Caches` holds its three defaults, read on the first
  question the scope is asked about a device space and shared by every render
  of it, since `for_one_render` shares `caches`. The read is one
  `/ColorSpace` lookup and three keyed gets; an entry that is present is
  parsed once, as written, and checked against the rules above. A tiling
  cell's scope is built anew per tile, so its read is once per tile, beside
  drawing the cell.
- **Per selection on the resource side**, a device arm of `color_space` or
  `parse_space` reads the cached answer: on a page with none, a load and a
  branch.
- **Per operator in the interpreter, nothing.** `FontSource::device_defaults`
  is asked once per scope — when `interpret` starts and when a form changes
  scope, restored when it ends — and answers which of the three device spaces
  the scope remaps. Its default answers none, so `recode.rs` and the tests'
  sources behave as with no defaults, and `subset.rs`'s `Recorded` forwards it
  as it forwards the rest of the colour seam. With none remapped, `g`, `rg`
  and `k` convert in place behind one branch on a field the interpreter
  holds; with one, that operator resolves through `resolve_color` and
  `resolve_ink` under the device space's own name, the way
  `/DeviceGray cs … sc` resolves.

#### The selections a default rides on

`the_selections_a_default_rides_on_read_what_the_file_names` holds three
selections a default rides on, with no default anywhere:

- **`sc` after a device operator** reads in that operator's family (above).
- **An inline image's `[/I …]` is `/Indexed`** (Table 94), though 8.9.7
  abbreviates the key `/Interpolate` as `/I` too (Table 93);
  `expand_inline_abbreviations` tells the family from the key.
- **An inline image may name a `/ColorSpace` resource** (8.9.7), and draws in
  that space.

A JPEG or JPEG 2000 image reads its `/ColorSpace` as a sampled image does,
so `default_spaces.rs` holds both to twins like every other path, in grey and
RGB, the families the encoder and the decoder's fixtures have.

## Milestones

1. `Group.space` resolved: a form's `/Group /CS` and the page-level `/Group`
   read.
2. Group-space compositing: per-shape buffers, complemented separable blends,
   conversion at the three boundaries.
3. `icc::Profile::parse`.
4. `Transform` for matrix/TRC profiles.
5. LUT profiles.
6. `ColorSpace::Icc` wired.
7. Known-answer tables.
8. Corpus movement: the `iccbased` count in `corpus/report.json`, and
   [render-verification](render-verification.md)'s metamorphic rows over the
   files carrying profiles.
9. K-02.2: the scope's default substituted on every painting path that reads
   a space, and the three selections it rides on.
10. K-02.3: extraction reports the space the samples are in.
11. K-02.4: the census, by scope.
12. K-02.5: the census's first figures.
13. RC-01.2: an intent selects its table in `tinker-pdf-color`
    ([Rendering intents](#rendering-intents-8658)).
14. RC-01.3: absolute colorimetric, by the media white.
15. RC-01.4: `D2B*` tags, before the `A2B*` tag of their intent.
16. RC-01.5: the intent in force reaches every flat colour.
17. RC-01.6: images in their intent, `/Intent` read.
18. RC-01.7: shadings and meshes in the intent in force, and the intent page
    among the fingerprints.
19. RC-01.8: the intent page's speed row in the benchmark set.

### Three decisions, and why

**No `RenderWarning` for a refused profile.** Ruling 10 wants leniency
reported so that "it opened" and "it opened cleanly" stay distinguishable. A
refused profile is drawn by component count, which is what this engine draws
without the profile: the reading is not a degradation from anything, and a
warning that fires per page would appear on documents whose colours are no
different for it. The refusal is a typed `IccError` the caller matches on.

**The two `bounds_ledger.rs` rows.** `MAX_ICC_TAGS` (1 024) and
`MAX_ICC_BYTES` (2 MiB) each carry the ledger's table of figures in the
constant's own doc and name a dedicated `#[test]` in `icc.rs`
(`a_tag_count_past_the_cap_is_refused_before_the_table_is_walked`,
`a_profile_past_the_byte_cap_is_refused_before_its_signature_is_read`). What
the rows hold beyond the refusal is the ordering each cap depends on: the byte
cap fires before the `acsp` signature is read, and the tag cap before the
`12 * count` table it describes is walked, so a 132-byte profile declaring
four billion tags is refused rather than indexed into.

Nothing in this engine reads a PNG `iCCP` chunk or a JPEG `APP2` profile, so
a comic page and a book picture carry their profiles past this parser without
them ever being opened. A fixed document is different, and the reason is not
obvious from `xps/profiles.rs`, which reads only §7.2's header through
`icc::data_space` and never calls `Profile::parse` at all: a `ContextColor`
profile part is embedded **verbatim** into the synthesised PDF as an
`/ICCBased` space, and it is *rendering that document* — `Resources`'s
`ICCBased` arm — that hands it to the parser. So the XPS route reaches these
caps by the ordinary PDF road, one pass later than it looks.

**`chad` is not read.** A matrix profile's `rXYZ`/`gXYZ`/`bXYZ` columns are
already relative to the D50 connection space — that is what makes them
addable — so applying the chromatic-adaptation tag on top would adapt a second
time and shift every colour. It is read by profiles that need to recover the
*unadapted* primaries, which nothing here does.

## Dependencies

- `tinker-pdf-math` (`pow`, `cbrt`, `ln`) and the `cargo xtask libm` gate over
  `PIXEL_PATHS`.
- The `parse_space` / `parse_function` resource seam in
  `crates/tinker-pdf/src/resources.rs`; the `open_group` machinery and
  budgets in `crates/tinker-pdf-render/src/lib.rs` (reused, not forked).
- The fingerprint suite (`crates/tinker-pdf/tests/determinism.rs`) and the
  bounds ledger (`crates/tinker-pdf/tests/bounds_ledger.rs`).
- [render-verification](render-verification.md)'s metamorphic rows, for
  milestone 8.
- [H-135](../roadmap/H.md), an open question, for milestone 9, which builds
  its default: an ink render keeps a `k` colour's own components under
  `/DefaultCMYK`. Milestone 12's corpus run depends on MS-11, as K-02.5
  lists it.
- K-01's stored intent, `StoredParameters::rendering_intent` in
  `crates/tinker-pdf-content/src/state.rs`, for milestones 16 to 18, which
  read it.

## Risks

| Risk | Mitigation |
| --- | --- |
| A platform transcendental sneaks into per-pixel evaluation and rendering diverges across targets | Evaluation is integer/fixed-point by construction; `cargo xtask libm` fails the build on any transcendental in `tinker-pdf-color`; the committed fingerprints are measured on all four targets |
| Hostile profiles: absurd tag counts, CLUT grids sized to exhaust memory, overlapping tag data | Ruling 1 discipline — the `icc_profile` fuzz target; profile size and tag count are measured rows in `bounds_ledger.rs` and CLUT volume is a named refusal without one; over-bounds is a typed refusal to `Approximated`, never an allocation |
| CMYK group buffers (5 bytes/pixel) inflate memory on group-heavy pages | Buffers are bounded to the group's device-space extent and `MAX_GROUP_BUFFERS` caps the count; the decline-and-warn path (`GroupBudgetSpent`) absorbs the excess |
| Non-separable blends in CMYK groups are an approximation and a reviewer mistakes it for a bug | The conversion is a named, warned limit (`RenderWarning::ApproximatedGroupBlend`) and stated in this doc |
| **No second CMM adjudicates this, under ruling 13.** A misreading of ICC.1 converts every profiled page wrongly, and every check here agrees with it | Matrix/TRC transforms are pinned bit-exactly against values hand-computed from named clauses, which is arithmetic rather than opinion. LUT interpolation has no such anchor and the doc says so; it is the largest named limit of this design |
| v2 versus v4 profile differences (PCS encoding, curve types) handled subtly wrongly | Known-answer tests in `icc.rs`, each expected value computed from the clause it comes from |
| A silence the default-space design decides — a form group's scope, the initial colour after `cs`, a default's own spaces, a refused default — is later read the other way | Each is stated as this design's decision rather than the standard's, and a test in `default_spaces.rs` holds it, so a different reading moves one test and one function |
| A page's default stops at a form with `/Resources` of its own, where a reader that carries defaults inward draws the substitute | The clause's words are followed ("the current resource dictionary"; 8.10.1); the census counts the files where the two readings part, and roadmap row K-02.5 commits its first figures |
| The device operators, the most frequent in a content stream, start paying for the seam | A scope's defaults are read once and the interpreter branches on a field it holds |
| H-135 is decided for conversion | One change to `resolve_ink`; `an_ink_render_keeps_a_device_cmyk_colour_s_ink_under_default_cmyk` names the contract it holds, so the change shows where it lands |
| A twin comparison passes with nothing applied and is taken for proof | Every twin is paired with the page without its default, which it must not equal |
| An intent converts through a table the profile's author did not make for it | Only the last column of the intent table is this design's, and it is reached only where ICC.1 names nothing the profile carries; `an_absent_intent_table_falls_back_as_icc_1_says` pins every row |
| A colour set before `ri` keeps the old intent's light | An intent change re-resolves the fill and stroke colours from the space and components kept beside them; `ri_and_extgstate_ri_are_twins` changes the intent after `scn` |
| Compiling per intent multiplies what a profile costs | A table is read and compiled on first use, one transform a distinct table; a page that sets no intent parses and compiles what it does today |
| A profile carrying a matrix or `grayTRC` and `A2B*` tables converts by its matrix, where ICC.1 8.10.2 converts by its tables | Stated as this design's departure, with the owner deciding whether to follow 8.10.2; until then every such profile renders as it does today, whatever the intent |

## P-22 — Colour conversion throughput

*For [ROADMAP](../ROADMAP.md) row P-22.* A colour converts through an ICC
space by running its transform's whole pipeline (Stage 2), and for a
lookup-table transform that pipeline is the slow one: `Lut::evaluate` takes
three allocations and up to sixteen corners of the profile's grid, then the
connection space goes to sRGB in floating point and each channel is encoded,
for a Lab connection space through `tinker_pdf_math::pow`. An image pays that
at every pixel `Remembered` has not seen. This section fixes which transforms
an integer grid stands in for, its size over each input count, what a node
holds and how a grid is read, the bound it is held to against today's
pipeline, which conversions read it and when its nodes are evaluated, how a
fill and an image pixel of one colour stay alike, the 8-bit row path, the
per-image converter's arms, the document cache's key and its charge to
P-16's budget, how hostile profiles are bounded, and the fingerprint page.
[The price of an integer grid](#the-price-of-an-integer-grid) is what P-22.1
priced; its table's figures are the end phase's, and the choices below rest
on what each form and size computes rather than on them.

### Scope

- **The grid** (P-22.4): each lookup-table transform over one, three or four
  inputs read through an integer grid of its own pipeline's light, in fixed
  point, its nodes evaluated at first read.
- **The row path** (P-22.5): a row of 8-bit samples converted to sRGB bytes
  through per-axis tables, with no float.
- **The per-image converter** (P-22.6): one converter chosen per image by its
  space, depth and `/Decode`.
- **The document cache** (P-22.7): transforms kept per document under the key
  below, their grids charged to P-16's budget.
- **The fingerprint page** (P-22.3) and **the close** (P-22.8).

### Non-goals

- **A grid for a matrix/TRC or a grey transform.** Its pipeline is already
  integer and exact (below); a grid would only add error.
- **A grid over two, or five to sixteen, inputs.** Such a table converts as
  today, on its own pipeline.
- **Another reading of a profile's own table.** `Lut::evaluate`, its 8-bit
  fractions and its multilinear interpolation stay as they are: the grid
  samples the pipeline, it does not replace any stage of it.
- **Intrinsics or `unsafe`** (H-16's default): the loops are plain integer
  code a target may autovectorise.
- **A transform shared across documents** (P-16's non-goal).
- **Destinations other than sRGB, proofing and black-point compensation.**
  RC-06, RC-05 and RC-02 bring them; each is a stage the grid's nodes
  evaluate and a value of the cache's key (below), never a per-pixel step.

### Design

#### Which transforms a grid stands in for

A lookup-table transform — `mft1`, `mft2` or `mAB ` at an `A2B*` tag, and
RC-01.4's `D2B*` elements when they read — over one, three or four inputs
converts through a grid. A matrix/TRC transform and a grey one keep their
pipeline: its curves are already sampled into 4 096-entry tables, its matrix
is s15.16 and its encoding a table, so a conversion is integer work and exact,
and a grid would add the error between its nodes and save nothing. That error
is largest exactly there: P-22.1's matrix/TRC rows at gamma 1.0 put a linear
tone curve under sRGB's toe, whose slope at black is 12.92 times linear
light's (IEC 61966-2-1), and a chord between two nodes of encoded output
misses that curve by what the slope multiplies. A table over any other input
count stays on today's path.

#### The grid's size

- **One input: 256 points**, so every 8-bit input is a node and a row of bytes
  converts exactly, at 256 evaluations.
- **Three inputs: 33 points**, 35 937 nodes. Against 17 points it halves a
  cell's width, which quarters a chord's error, for a grid a thirty-third of
  the one 33 points over four would be.
- **Four inputs: 17 points**, 83 521 nodes. 33 points over four would be
  1 185 921 nodes, more than twice the 543 740 pixels of MS-04's 620 × 877
  CMYK image, so a grid that fine costs more to fill than the conversions it
  stands in for; 17 points fill in under a sixth of that image's pixels.

#### What a node holds

The pipeline's light at the node before it is encoded: the linear sRGB its
connection space converts to, each channel a signed 21-bit integer in which
65 535 is 1.0, so from −16 up to 16, beside a flag saying the node is filled —
one 64-bit atomic word a node, filled with no lock.

- **Light, not the encoded byte P-22.1's probe holds.** A printer profile
  reaches black, where the encoding is steepest, so between two nodes near
  black a chord of encoded bytes bends where the light does not; the light is
  the quantity the inks scale, and a chord of it follows a profile's table far
  more closely there.
- **Unclamped.** A node outside the gamut keeps its distance from it, so a
  chord across the gamut's edge stays straight in light and is clamped after
  it is read, as the pipeline clamps after it computes.
- **The node reads back the pipeline's byte.** Where a channel's light is
  within 0..=1, its integer is the one nearest the light among those the read
  encodes to the pipeline's own byte for that channel (every byte is reached,
  since the encoding table steps by under a level), so a grid read at a node is
  the direct pipeline exactly, as P-22.1's probe asserts of its grids.

#### How a grid is read

- **Locating.** Each component is clamped to 0..=1, then
  `round(x × (points − 1) × 65 536)` is split into its cell (the high bits)
  and a 16-bit fraction; the top of the last cell reads as that cell at
  65 536. A missing component reads as 0, as the pipeline reads one, and a
  NaN locates at 0.
- **One input** reads linearly between its two nodes. **Three** read
  tetrahedrally: from the cell's lowest node a step along each axis in turn,
  the largest fraction first and the earlier axis first between equals, each
  node weighed by its step's fraction against the one before. **Four** read
  tetrahedrally over the first three inputs at the two nodes of the fourth
  either side of it, then linearly between those two — P-22.1's
  "tetrahedral then linear".
- **In i64, rounded once** to the light's integer, clamped to 0..=65 535 and
  encoded through a 65 536-entry table of the pipeline's own encoding, one for
  each connection space and built once: under Lab the pipeline's
  `srgb_encode`, under XYZ its `encode_component`, each sampled at the
  table's 65 536 values. So the read encodes as the pipeline encodes, and the
  interpolation between nodes is all that is new.
- **Why that form over four.** Black ink scales the light the other three
  leave, so within a cell a light that is a function of C, M and Y times one of
  K is linear in K, which the blend in K reproduces and the 4-simplex, cutting
  each C–K square on its diagonal, does not; and it reads eight nodes where
  multilinear reads sixteen.

#### The bound

Every node is the direct pipeline, exactly. Over one input every 8-bit input
is a node, and a component between two — a fill's `0.3` — is within one level
of 255 of the pipeline. Over three and four inputs, every input P-22.1's sweep
reads is within **two levels of 255** of the direct pipeline. P-22.4's test
holds both for each lookup-table model P-22.1 priced, and P-22.1's probe gains
the transform's own grid as a form of its own, so its table prices what is
built beside what it priced; the test's failure prints the worst input it met.

#### Which conversions read it, and when its nodes are evaluated

- **Every conversion through a gridded transform reads its grid**:
  `Transform::apply`, and with it every `ColorSpace::to_rgb_in` through an ICC
  space — fills, strokes, glyphs, stencil masks, shading samples and the
  colour tables P-14 builds from them, `/Indexed` palettes, tint alternates and
  `/BC` — and the row path. Nothing reads the pipeline directly but a node's
  evaluation and the tests' reference, `Transform::evaluate`, which is what
  `apply` is today.
- **A node is evaluated the first time a conversion reads it** and kept with
  the transform for every conversion after; the node array is allocated at the
  first conversion through the grid. So a page painting one colour through a
  printer profile evaluates the eight nodes that colour reads, where filling
  the grid when the transform compiles would evaluate all 83 521 at every
  render that compiles it — once a render today, once a document after
  P-22.7 — and an image evaluates each node its pixels read once.
- **What a conversion returns never depends on which nodes were filled before
  it**: a node is a function of the profile, the conversion and its position,
  so two threads filling one node at once store one value, a tile reads what
  its page reads (ruling 5), and a hit in P-16's cache reads what its miss
  computes. Once every node is filled, no conversion evaluates the pipeline.

#### A fill and an image pixel of one colour

One function locates a component, and every conversion of a gridded
transform reads the grid through it, so a fill whose components are `k / 255`
and an image sample `k` read the same nodes at the same fractions and come out
the same bytes; the row path's axis table is that function evaluated at
`k / 255` for each byte `k`, so it holds by construction rather than by
agreement. A matrix/TRC or grey transform's row path is its pipeline's own
arithmetic, so its fill and its pixel are equal for the same reason.

#### The row path

`Transform::convert_row` converts a row of 8-bit samples, one, three or four a
pixel, straight onto the end of a buffer of sRGB bytes:

- **A gridded transform** locates each sample through a 256-entry axis table
  of cell and fraction, one a transform since every axis has the same points,
  built at its first row; then the read above, in integer loops over
  `chunks_exact`, the node array's length and the cell strides hoisted, with
  no float, no allocation and no call of `apply`.
- **A matrix/TRC transform** indexes each channel's curve through a 256-entry
  table of the entry `apply` reads for that byte, already in s15.16, then
  multiplies and encodes as `apply` does, term for term.
- **A grey transform, and a grid over one input,** read a 256-entry table of
  the three bytes `apply` gives each byte.
- **Any other input count** converts a pixel at a time through `apply`.

#### The per-image converter

P-22.6 chooses one converter an image, by its space, depth and `/Decode`,
where the sample loops call `to_rgb` per pixel today:

| Image | Converter |
| --- | --- |
| 8-bit DeviceRGB, and DeviceGray, `/Decode` absent or `[0 1]` | a copy, a grey replicated to three (P-08's identity path) |
| Any one-component space at up to 8 bits, DeviceGray among them | a table of at most 2^bpc colours, built through `to_rgb` |
| 8-bit DeviceCMYK, `/Decode` absent or `[0 1]` | one table of 8.6.4.4's `(1 − c)(1 − k)`, 256 × 256 bytes built through `to_rgb`, read for each of C, M and Y against K |
| 8-bit ICC, `/Decode` absent or `[0 1]`, no colour key | the row path |
| Everything else | `to_rgb` over one reused buffer, with `Remembered` |

A table built through `to_rgb` is each device pixel's byte by construction,
and the row path is `apply`'s, so the converter moves no pixel.

#### The document cache: its key and its charge

P-16.1's compiled-ICC kind, re-keyed by P-22.7:

- **The entry** is one profile's `Transforms` toward one destination, keyed by
  the profile's bytes — their SHA-256 for the map, the bytes compared in full
  on a hit, so a profile embedded as many objects is one entry — and by the
  destination, sRGB, the only one this engine converts to.
- **Inside it** `Transforms` keeps one compiled transform for each
  `Conversion` asked of it, today one an intent, so a transform's whole key is
  the profile's bytes, the destination and the conversion: the row's
  "(profile pair, intent, compensation)", which the guard counts.
- **What later rows add.** RC-02's black-point compensation is a field of
  `Conversion` ("The one conversion parameter"), RC-01's intents are its
  values already, and RC-06's destination profiles are values of the
  destination, keyed by their bytes as the source is: each adds keys and none
  reshapes the kind.
- **A refusal is kept** under the profile's bytes, so a profile that does not
  parse is refused once a document.
- **The charge**, taken when the entry is inserted: the parsed profile's
  tables — its `A2B*` elements as `Profile::parse` holds them, or a
  matrix/TRC transform's three 4 096-entry curves — and, for each distinct
  table its conversions can select, a grid's whole node array at eight bytes a
  node, with P-16.1's 64-byte floor. That is the most the entry can come to
  hold, charged before any node is filled, so filling never passes the
  budget. An entry the budget cannot hold is used by the request that made it
  and not kept, so past the budget a transform compiles per use, as today.

#### Hostile profiles (ruling 1)

A grid exists only over one, three or four inputs, and a table over any other
count converts as today. A node's evaluation is the pipeline once, whose work
is bounded by its input count — at most sixteen corners over four inputs —
and not by how large the profile's own table is, so a grid's whole build is
at most 83 521 evaluations and its memory at most 668 168 bytes whatever the
profile declares, under a third of `MAX_ICC_BYTES`, the most a profile itself
may be. The `icc_profile` fuzz target compiles and applies every intent, so a
hostile profile builds its grid under it, and P-22.5 has it convert a row of
every byte value through each transform it compiles.

#### The fingerprint page

`determinism.rs`'s `icc` page (P-22.3) draws an 8-bit CMYK image through a
four-channel `mft2` profile over a Lab connection space, an RGB image and a
fill through a matrix/TRC profile, a grey-profile image and a DeviceCMYK
image beside them, each profile built in the test, and is recorded from
today's per-pixel pipeline. The CMYK image is its only gridded conversion, so
P-22.4 moves the page by that image's pixels or not at all, and the rest of
the page is the control: a move outside the image is a defect, not the grid.
`default_spaces`, whose `/DefaultCMYK` is an `mft2` profile into XYZ, is the
other pin P-22.4 may move, and H-76 reviews the two moves together.

### Milestones

1. **P-22.1: the price.** `the_grid_prices_the_design_quotes` and
   `crates/tinker-pdf-color/data/icc_grid_prices.tsv`.
2. **P-22.2: this section.**
3. **P-22.3: the `icc` page.** Exit: `determinism.rs`
   `rendering_is_stable_across_targets` with the new `icc` entry, recorded from
   today's per-pixel pipeline, under `cargo test` on linux and windows and
   under `--target wasm32-wasip1`; `banded_rendering_reproduces_every_fingerprint`,
   `svg_output_is_stable_across_targets` and
   `a_display_list_replays_every_fingerprinted_page_byte_for_byte` hold it too.
4. **P-22.4: the grid.** Exit: `icc.rs`
   `the_grid_is_the_pipeline_at_its_nodes_and_within_the_bound_between` — for
   each lookup-table model P-22.1 priced, every node the direct pipeline
   exactly and the sweep within the bound, and the matrix/TRC and grey models
   converting as their pipeline does, with no grid;
   `a_conversion_evaluates_no_pipeline` — pipeline evaluations made by
   conversions, none once every node is filled and at most one a node before;
   `icc.rs`'s known-answer tests, `colour_spaces.rs`'s ICC tests and
   `cargo xtask libm`; the `icc` and `default_spaces` fingerprints, if they
   move, re-recorded once after H-76's review, the four targets agreeing.
5. **P-22.5: the row path.** Exit: `icc.rs`
   `a_row_of_bytes_converts_as_apply_does` — every 8-bit input of a one- and a
   three-input transform of each model, and a 17-level lattice of a four-input
   one, converting to the bytes `apply` gives; and
   `a_row_reads_its_axis_tables_and_no_float` — axis-table builds a
   transform, one, and `apply` calls a row, none; `cargo xtask libm`.
6. **P-22.6: the converter.** Exit: `image_conversion.rs`
   `a_converter_is_chosen_once_per_image` — `to_rgb` calls an image (MS-01's
   tally) not growing with its pixels, allocations an image O(1) (A10), each
   device arm's bytes equal to per-pixel `to_rgb` over every input; the
   image, `jbig2`, `jpx` and `icc` fingerprints unchanged.
7. **P-22.7: the document cache.** Exit: `icc_cache.rs`
   `transforms_built_are_at_most_the_distinct_keys` — a generated 20-page
   document naming one RGB and one CMYK profile through a different object on
   every page, in a form and in an appearance stream, rendered twice, builds
   two transforms; under a budget below one grid every page byte-identical;
   the `icc` page rendered twice through one document hashing to its
   fingerprint both times.
8. **P-22.8: the close**, in MS-02's paired run: MS-04's 620 × 877 CMYK image
   at its calibrated target, at least 100 Mpixel/s, with the row's guards green
   on every target. The operation draws its image under
   `fuzz/corpus/icc_profile/cmyk-lut.icc`, a seed `write_the_fuzz_seeds`
   builds to be refused (`NeedsLut`), so until that operation's profile is one
   that compiles it times 8.6.5.5's component-count fallback and no
   transform; P-22.8 gives it a four-input `mft2` profile, and its renders
   read one grid only through P-22.7's cache under a stated budget.

### Risks

| Risk | Mitigation |
| --- | --- |
| The grid misses a real profile by more than the bound | The bound is held on every lookup-table model P-22.1 prices, the probe prices the built grid beside its own forms, and the fingerprint moves are the owner's to review (H-76) |
| A fill and an image pixel of one colour part | One locating function serves both, the axis table is built from it, and P-22.5's test holds every byte to `apply` |
| What a page draws depends on which nodes were filled | A node is a function of the profile, the conversion and its position; filling is idempotent and order-free, so tiles, threads and cache hits read the same values |
| A page with one ICC colour pays for a whole grid | Nodes are evaluated at first read; a colour costs the nodes it reads |
| A hostile profile builds an unbounded grid | A grid exists over one, three or four inputs only, at most 83 521 nodes of bounded work and 668 168 bytes; the fuzz target builds them |
| A matrix/TRC page moves though nothing was gained | Matrix/TRC and grey transforms build no grid; the `icc` page's matrix/TRC and grey parts are its control |
| Filling a grid passes P-16's budget | The entry is charged its whole node arrays at insertion, before any node is filled |
| Until P-22.7, every render rebuilds what it reads | Each render's transforms fill only the nodes it reads, which is at most what today's per-pixel pipeline evaluates for a page of distinct colours, and P-22.7 keeps them per document |
| The close times no profile at all | P-22.8's milestone names the refused seed and gives the operation a profile that compiles |
