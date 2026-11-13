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
  (`rXYZ`/`gXYZ`/`bXYZ`, `wtpt`), LUT transforms (lut8 `mft1`, lut16
  `mft2`, v4's `mAB `) read from a profile's `A2B*` tags, and the
  floating-point multi-process transforms (`mpet`) of its `D2B*` tags,
  compiled into a profile→PCS→destination pipeline. `chad` is not read (see
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

- **Output intents outside proofing** (14.11.5): judging an intent against
  the part it was claimed under belongs to [pdfa](pdfa.md) and
  [pdfx](pdfx.md). An output intent's profile as the device a page is
  rendered for, and a caller's profile in its place, are
  [Soft proofing](#soft-proofing-rc-05) below; without one, every render
  converts for the screen, to sRGB.
- **N-channel output, overprint and spot-ink simulation.** `Separation` /
  `DeviceN` keep flowing through their tint transforms (8.6.6.4, 8.6.6.5);
  simulating ink interaction is a different renderer. Overprint simulation and
  separations are [ROADMAP](../ROADMAP.md) rows RC-03 and RC-04, designed in
  [overprint-separations.md](overprint-separations.md).
- **Black point compensation, gamut mapping beyond intent selection,
  iccMAX/v5 profiles, named-colour tags.** Refused by name, per ruling 2;
  [ROADMAP](../ROADMAP.md) rows RC-02 and RC-20, the second designed in
  [ICC engine completion](#icc-engine-completion-rc-20).
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
kind (matrix/TRC, grey, LUT), device class and version; the structure at
every table tag (`A2B*`, `B2A*`, `D2B*`, `B2D*`, `gamt`); and the profiles
of class `link` and `nmcl`, those carrying `gamt` or `ncl2`, and those of
version 5 with each tag's structure.
`what_the_parser_makes_of_the_corpus_profiles` then hands them to the parser
and holds its answer as a ratchet: `STREAMS` exact, being the lock file's
population, `COMPILED` a floor and `REFUSED_SHAPES` a ceiling, so a run that
compiles more prints the gain to record, and one that compiles fewer, or
refuses a profile in a shape not listed, fails. Each refused profile is
printed whole: its version, class and spaces, and each tag with its
structure. `a_census_that_compiles_more_is_a_gain_and_less_a_failure` holds
the comparison on every run, the corpus fetched or not. A file that carries
a profile stream is not a file that paints through an `ICCBased` space: a
PDF/A file's `/OutputIntent` declares the space it was prepared for whether
or not any content is painted through it.

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
profiles a grid interpolated over its axes in integers: over up to six
inputs as one weighted sum of its corners, whose 64-bit sums hold
`256^6 × 65 535`, and over more — a seven- or eight-ink profile an XPS
document embeds — one axis at a time. Sampling a `para`
curve needs `pow` — that is `tinker_pdf_math::pow`, exactly as `lab_to_rgb`
uses it, because `tinker-pdf-color` is on the `PIXEL_PATHS` list that
`cargo xtask libm` enforces and a platform `powf` in a TRC is precisely the
cross-target divergence ruling 4 exists to stop. The PCS leg reuses the D50
XYZ→sRGB path; a LUT profile's connection space is read both as XYZ and as
Lab in the encoding its table type carries ([The connection space each table
type encodes](#the-connection-space-each-table-type-encodes)), which goes
through the `finv` machinery `lab_to_rgb` owns. A grey profile compiles its one curve to a luminance the
three sRGB channels share. A lookup-table transform over one, three or four
inputs is read through an integer grid of that pipeline, its nodes evaluated
as conversions reach them ([P-22](#p-22--colour-conversion-throughput));
`Transform::evaluate` is the pipeline itself, which each node and the tests
run. `Transform::convert_row` converts a row of 8-bit samples to sRGB bytes
through tables of 256 entries built at a transform's first row, each pixel
the colour `apply` gives its components.

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
  bit-identical contract across targets: `default_spaces`, a page under CIE and
  `ICCBased` default spaces, and `icc`, whose pictures and fill convert through
  an `mft2` table into Lab, a matrix/TRC profile and a grey one, beside a
  `/DeviceCMYK` control.

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
`Profile::parse` keeps each `A2B*` table a profile carries beside its
matrix/TRC or grey model, and `ColorSpace::to_rgb_in` converts through the
one an `icc::Conversion`'s intent selects, by the table below. A flat colour
converts in the intent in force at its painting operator: the interpreter
passes the state's intent through `FontSource::resolve_color_wide`, which
answers `resolve_color`'s bytes with their 16-bit light, and the facade
converts in the `icc::Conversion` the content crate maps it to
(`RenderingIntent::conversion`). An image converts in its own `/Intent`, or
in the intent in force at `Do` or `EI` where it names none: the renderer
hands the state's intent through the image seam (`GlyphSource::image` and
`inline_image`), and the facade reads the image's own entry. A shading
converts in the intent in force at its painting operator — `sh`, or the fill
or stroke that paints a shading pattern — a mesh's vertices among them: the
renderer converts a shading's colours itself, through the `ShadingScratch`
its sweep keeps, which carries that conversion and builds the sweep's colour
table through it. How often
the corpus names an intent is not measured; an ExtGState names one by
`/RI`, the graphics state parameter dictionary having no `/RenderingIntent`
key. This section is the design RC-01's steps build, read
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

| Intent | 8.10.2 a), from RC-01.4 | b) | c) | d) | then | Media white |
| --- | --- | --- | --- | --- | --- | --- |
| `Perceptual` | `D2B0` | `A2B0` | — | the model | `A2B1`, `A2B2` | — |
| `RelativeColorimetric` | `D2B1` | `A2B1` | `A2B0` | the model | `A2B2` | — |
| `Saturation` | `D2B2` | `A2B2` | `A2B0` | the model | `A2B1` | — |
| `AbsoluteColorimetric` | `D2B3` | `A2B1` | `A2B0` | the model | `A2B2` | scaled, except through `D2B3` |

"The model" is d)'s tone curves and colorant columns, or its `grayTRC`: a
profile's matrix/TRC or grey model.

- **Read across, first present wins.** The last column of tags is this
  design's, not ICC.1's: a profile that carries none of the tags 8.10.2
  names for an intent, and no model that reads, is read through its other
  tables in the order `Profile::parse` reads them, so no intent refuses a
  profile another intent converts.
- **Absolute colorimetric is relative colorimetric's table, scaled**, where
  no `D2B3` is read: 6.2.3 generates it from the media-relative transform,
  and 8.10.2 b) for it is `AToB1`. `D2B1` is not in its row: 8.10.2 names
  only `DToB3` for the absolute intent.
- **A tag the engine cannot read is passed over, not refused**: a `D2B*` tag
  holding an element this build does not read is unsupported in 8.10.2 a)'s
  sense — refused by the element's name, `IccError::UnsupportedElement` — and
  the intent falls to its `A2B*` tag. So is an `A2B*` table that does not
  read, and so is a model that contradicts the profile's header — colorant
  columns over a data space that is not RGB, a `grayTRC` for one that is not
  grey, a connection space the model cannot reach — which the next source in
  the row serves.
- **An intent whose row finds nothing converts as relative colorimetric
  does.** Every row holds every `A2B*` table and the model, so only a profile
  whose one source is another intent's `D2B*` tag reaches this: its intents
  take relative colorimetric's `D2B1`, absolute colorimetric scaling it as
  it scales any media-relative transform.
- **A profile is refused only where relative colorimetric finds nothing.**
  `Profile::parse` finds `RelativeColorimetric`'s source, 8.6.5.8's initial
  intent, and refuses a profile with none, by its model's reason where it
  carries one. A profile carrying no model is refused for the first table
  that row names when that table does not read — in a profile of tables
  alone, a damaged table is a damaged profile — and one carrying a model is
  never refused for its tables.
- **Tags at one offset are one table** (8.2): read once, compiled once, and
  shared by every intent that selects it.
- **Matrix/TRC and grey profiles** (8.10.2 d)) serve every intent that
  reaches the model from one transform, whatever the profile's class, and
  `AbsoluteColorimetric` from that transform with the media white folded in.
- **A profile carrying a matrix or a `grayTRC` beside `A2B*` tables converts
  through its tables first**, as 8.10.2 orders them: b) and c) before d), for
  every intent, so the model serves only an intent for which the profile
  carries neither its own `A2B*` tag nor `A2B0` — perceptual and saturation
  beside an `A2B1` alone, for one. Every page painting through such a profile
  renders through its tables, not through its model; the owner took that
  (ruling 23, the RC-01.9 row), and RC-01.9b counts the corpus profiles it
  moves (`icc_census.rs`).

#### The `D2B*` tags

A `D2B*` tag is a `multiProcessElementsType` (ICC.1:2010 10.14): a chain of
processing elements in float32, read by `crates/tinker-pdf-color/src/icc/mpe.rs`
and compiled once into integer stages, so a colour's evaluation stays
integer arithmetic (ruling 4).

- **What is read.** A curve set (`cvst`) of segmented curves (`curf`), each
  segment a formula (`parf`, Table 57's three: `(aX + b)^γ + c`,
  `a log10(bX^γ + c) + d` and `a b^(cX + d) + e`) or samples (`samf`), the
  first and last a formula; a matrix with offsets (`matf`, equation 21); a
  lookup table (`clut`), inputs held to `0..=1`; and the `bACS` and `eACS`
  placeholders, which pass their channels through. The tag's output is the
  connection space in its float encoding (6.3.4.2): XYZ with the PCS white's
  `Y` at one, or `L*a*b*` as it stands.
- **How it is compiled.** Each stage knows the interval its inputs can take,
  starting from the device's `0..=1`: a curve is sampled at 4 096 evenly
  spaced points over its channel's interval, its formulas evaluated with
  `tinker_pdf_math`, and interpolated linearly between them; a matrix and a
  grid are held in s15.16, the grid interpolated multilinearly one axis at
  a time. A stage's outputs give the next one's intervals — a curve's
  samples, a matrix's rows over its columns' intervals, a grid's nodes — so
  nothing is clipped between elements (10.14.2.1) beyond s15.16's range.
- **What is refused.** An element of another type, by its signature
  (`IccError::UnsupportedElement`), whatever else the tag holds, since
  10.14.1 makes such a tag unusable; a number that is not finite, a curve
  that is not finite over the inputs it is given, a break-point that
  decreases, a channel count that does not chain; and every count before it
  is allocated: the element count to `icc::MAX_MPE_ELEMENTS`, channel counts
  to sixteen, a grid to `MAX_CLUT_ENTRIES`, and the grid and curve entries a
  tag compiles to `MAX_CLUT_ENTRIES` in all. Each refusal passes the tag
  over, so the intent converts through its `A2B*` table.
- **How a colour converts.** Over one, three or four inputs, through a grid
  of the elements' light, as a table does
  ([P-22](#p-22--colour-conversion-throughput)): each node the stages and the
  connection space into linear sRGB, evaluated the first time a conversion
  reaches it. Over any other count, through the stages for each colour.

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
  `L*` with `a* = b* = 0`, the same white), scaled, is `Y × wtpt`, and the
  scaling is folded into the transform's matrix as one column,
  `XYZ_D50_TO_SRGB × wtpt`. A grey profile whose media white is not D50 draws
  its greys in the medium's tint under this intent; under the others it stays
  achromatic, its three channels one luminance.
- **A table** (`mft1`, `mft2`, `mAB `, and a `D2B*` other than `D2B3`): the
  PCS value it gives — XYZ, or Lab in its type's encoding taken to XYZ by
  the relation `lab_to_rgb` runs (`lab_to_xyz`) — scaled per channel, then to
  sRGB as the relative path does. The table is the relative intent's own,
  shared, and only the scaling after it is absolute colorimetric's: a stage
  the nodes of the transform's grid evaluate (P-22), so the absolute
  transform reads a grid of its own.
- **The PCS white is 6.3.4.3's**, taken as the s15Fixed16 numbers a
  profile's `XYZType` tags carry (`0x0000F6D6`, `0x00010000`, `0x0000D32D`,
  `icc::PCS_WHITE`), so a `wtpt` of D50 scales by exactly one and absolute
  colorimetric is relative colorimetric bit for bit: the two intents are one
  transform (`Profile::media_scale` is one in every channel).
- **A profile with no `wtpt`, one with a component that is not positive, or
  one whose `wtpt` does not read as an `XYZType`**, is taken to have the PCS
  white for its media white: 8.2 requires the tag, an `ICCBased` space's
  profile cannot be a DeviceLink (Table 67's classes), and taking the PCS
  white adds no colour the profile does not state. Its absolute colorimetric
  is its relative colorimetric. The media white is read for every model and
  refuses nothing: a grey profile with no `wtpt` compiles, its relative
  transform being achromatic without it.

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
  colour the space each was set in and its components
  (`GraphicsState::fill_source` and `stroke_source`, a shared
  `ColorSource`), and an intent that changes — by `ri` or by a `gs` with
  `/RI` — re-resolves both through `FontSource::resolve_color_wide` in the
  new intent, the bytes and the 16-bit light together. A colour a device operator converts in place keeps its light, and
  an uncoloured pattern's keeps its pattern. `Q` restores a colour with the
  intent it was resolved in; setting the intent in force again resolves
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
`Separation` alternate, a pattern's base. The content crate maps its
`RenderingIntent` to an `icc::Conversion` (`RenderingIntent::conversion`),
`Unrecognised` to `RelativeColorimetric`: the one mapping the facade's flat
colours and images and the renderer's shadings convert through, so the leaf
keeps no PDF name for an intent. RC-02 adds black-point compensation to
`Conversion` as a second field, and keys what this design keys by intent by
the whole value.

#### What compiling per intent costs

- **Parsing.** `Profile::parse` reads the tag table, the model where the
  profile carries one, and the tables relative colorimetric's row reaches
  until one reads; it keeps a copy of each other `A2B*` element's bytes and
  reads one when an intent first selects it. A page that sets no intent reads
  no table it does not convert through.
- **Compiling.** A space's transforms are kept together, each compiled the
  first time an intent selects its source, and shared by every conversion
  through that space and every page whose resources compile the profile. A
  profile used in one intent compiles one transform. The others compile one
  transform a distinct source, sharing the transform of an intent that
  converts through the same table or the same model: a profile used in all
  four compiles one for each distinct table it converts through and one for
  its model, and absolute colorimetric one of its own where the media white
  is not D50, its scaling applied after a table's evaluation or folded into
  the model's matrix.
- **Memory.** A LUT transform holds its grid, at most `MAX_CLUT_ENTRIES`
  16-bit entries, so a profile used in three intents holds at most three
  times what one intent holds; a matrix/TRC transform is three 4 096-entry
  curves and a matrix, and its absolute twin shares the curves.
- **Caches key on the intent's table.** The facade's ICC cache keeps a
  profile's transforms beside each other, so an intent changing finds the
  profile already parsed; the decoded-image cache keys an image's decode on
  the conversion its colours took beside the reduction it was made at, since
  one image drawn under two intents is two conversions. The first decode of a
  name records the image's own `/Intent` and whether any intent changes its
  colours — its space as painted holds an ICC profile and it is no image
  mask — so a later draw knows its key before it decodes: an image no intent
  changes is one decode under every intent, and one naming its own intent is
  one decode whatever intent is in force. A document's cache under a budget
  keys the decodes it keeps across renders the same way (`cache::ImageKey`),
  reading the image's `/Intent` and space before the lookup. An inline image
  is decoded at every draw, in the conversion it asks for.

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
- **RC-01.9**: a model beside tables, in 8.10.2's order.
  `a_profile_carrying_a_model_and_tables_reads_its_tables_first` (a
  matrix/TRC and a grey profile under each connection space, carrying each
  set of `A2B*` tables, every intent through b), c) or d)),
  `a_table_that_does_not_read_is_passed_over_to_the_model`,
  `a_profile_whose_matrix_contradicts_its_space_reads_through_its_table` (the
  census's three shapes, each with a table, converting as the table says),
  `intents_sharing_a_source_share_one_transform`, and the
  `model-and-table.icc` and `contradicting-model-and-table.icc` seeds in
  `write_the_fuzz_seeds`. RC-01.9b counts the corpus profiles carrying both
  in a local run of `icc_census.rs`.

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
  holds; with one, that operator resolves through `resolve_color_wide` and
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

### ICC engine completion (RC-20)

Roadmap row RC-20. The CMM above converts one way, from a profile's device
values to the connection space and on to sRGB, through the transforms
ICC.1:2010 (ISO 15076-1) defines for that direction. This section designs
the rest of the format: the transform the other way, from the connection
space to a device; device links; the gamut tag; named colours; and the
version 5 profiles of ICC.2:2023 (ISO 20677, iccMAX). Every layout and
encoding below is read from those two specifications, ICC.1 by its 2010
clause numbers and ICC.2 by its 2023 ones.

#### Scope

- **The output transform**, `icc::Output`: a profile's transform from the
  connection space to its device components, compiled for an intent by the
  precedence RC-01 reads (8.10.2 for `BToDx` and `BToAx` as for the forward
  tags), from the `B2D*` and `B2A*` tables or from the model inverted.
- **Each model's output**: a matrix/TRC and a grey model inverted; `B2A*`
  tables in `mft1`, `mft2` and `mBA `.
- **Device links**, class `link`: a device-to-device transform.
- **The gamut tag**, `gamt`: whether a connection-space value is in gamut.
- **Named colours**: `ncl2`, looked up by name; class `nmcl`.
- **Version 5**: ICC.2's header, multi-process elements in both directions
  with the curve forms and elements ICC.2 adds, the calculator element, and
  a spectral connection space reduced to a colorimetric one.
- **The Lab encodings** each table type carries, settled below, and the
  forward reading corrected where it is wrong.
- **The census ratchet**: `icc_census.rs` holds the compiled count as a
  floor and the refused shapes as a ceiling.

A profile whose model contradicts its header and which carries a table is
read through the table already (RC-01.9, [The table each intent
compiles](#the-table-each-intent-compiles)); one carrying no table it can
convert through stays `ColorSpace::Approximated`.

#### Non-goals

- **Rendering intents** (RC-01), **black-point compensation** (RC-02),
  **soft proofing** (RC-05), **converting a document** to an output intent
  (ED-06b) and **throughput** (P-22): each its own row, which this section
  supplies with transforms and does not drive.
- **What ICC.2 defines and no PDF reaches**: BRDF and directional tags,
  multiplex connection spaces (`AToM0`, `MToA0` and the `mid `, `mlnk` and
  `mvis` classes), extended-to-standard range tags (`HdrToSdr*`), gamut
  boundary descriptions (`gbd `), embedded images and the colour encoding
  space class (`cenc`). Parsed past: a profile carrying them converts by
  the tags this design reads, and one with nothing else is refused by name.
- **Spectra other than reflectance and transmission**: emission (`es`),
  bi-spectral (`bs`) and sparse-matrix (`sm`) connection spaces, and the
  elements that compute them — the emission and reflectance CLUT, matrix
  and observer elements, the colour appearance elements `JabToXYZ` and
  `XYZToJab`, sparse matrices, tint arrays and tone maps. Refused by name.
- **A standard observer named without its table** (ICC.2 10.2.22, Table X):
  refused by name. The table would be vendored data, and data enters only
  under a licence the allowlist admits (ruling 18); ICC.2:2023 grants none.

#### Design

##### The connection space each table type encodes

Every table reads its connection-space side in the profile header's PCS,
and the encoding is the tag type's, not the profile's:

| Tag type | PCSXYZ | PCSLAB | Clause |
| --- | --- | --- | --- |
| `mft2` (`lut16Type`) | `u1Fixed15`, `8000h` is 1.0 | legacy: `L*` 100 at `FF00h`; `a*`, `b*` 0 at `8000h` and 127 at `FF00h` | ICC.1 10.8, Tables 39 and 40 |
| `mft1` (`lut8Type`) | not defined, "implementation specific" | 8-bit: `L*` 100 at `FFh`, `a*`, `b*` 0 at `80h`; times 257, the 16-bit encoding of 6.3.4.2 | ICC.1 10.9, 6.3.4.2 Tables 12 and 13 |
| `mAB `, `mBA ` | `u1Fixed15` | each component's range scaled to `0..1`: `L*` 100 at 1.0 (`FFFFh`), `a*` and `b*` from −128 at 0 to 127 at 1.0 | ICC.1 10.10.1, 10.11.1, 6.3.4.2 |
| `ncl2` (`namedColor2Type`) | `u1Fixed15` | legacy, as `mft2` | ICC.1 10.15, Table 63 |
| `mpet` (`multiProcessElementsType`) | float32, the value itself | float32, the value itself | ICC.1 6.3.4.2, 10.14 |

So only `lut16Type` and `namedColor2Type` keep version 2's legacy scale, as
ICC.1 6.3.4.2's note 3 says. A table carries its scale (`Lut::lab_scale`):
an `mft2` decodes and encodes its Lab as legacy, and an `mft1`, an `mAB `
and an `mBA ` each component's range over the whole currency — the
`mft1`'s 8-bit encoding widened — in both directions. Read as legacy, a v4
table would be a factor of 65 535/65 280 off: `L*` 100 would read 100.39 and
`a*` 0 would read 0.5. `mft1` under PCSXYZ is read as its 16-bit widening in
`u1Fixed15`, the reading ICC.1 leaves to the implementation. A curve or grid
value outside `0..1` is clipped there, as 10.10.5 and 10.11.3 clip a v4
matrix's output.

##### The output transform

- **Surface.** `icc::Output::compile(&Profile, Intent) -> Result<Output,
  IccError>`, and `apply(&self, pcs: [f64; 3]) -> DeviceColour`: the
  connection-space value in the profile's own PCS, as real numbers (D50 XYZ
  with the PCS white's `Y` at one, or `L*a*b*`), in; the profile's data-space
  channels, each a `u16` from 0 to 65 535, out, at most `MAX_DEVICE_CHANNELS`
  of them. A component that is not a number is taken as zero and one past
  four as four, and light below zero or past one is held there. A caller
  holding the other PCS converts with `icc::xyz_to_lab` or `icc::lab_to_xyz`,
  6.4's relations against the D50 white the forward pipeline's Lab reads,
  the cube root `tinker_pdf_math`'s.
- **Which table.** By the forward rows of [The table each intent
  compiles](#the-table-each-intent-compiles), with `B` for `A`: the
  intent's `B2D*` (RC-20.10), its `B2A*`, `B2A0`, the model inverted, then
  the other `B2A*` tables; absolute colorimetric's value divided by the
  media scale before it enters, except through `B2D3`. A source that does
  not read — a table refused, a model with no inverse — is passed over, and
  where none reads the transform is refused for the first reason one did
  not, `MissingTags` where the profile carries none. `Profile::parse` keeps
  a copy of each `B2A*` element's bytes, as it keeps each `A2B*` table no
  intent has read yet, and a transform out of the connection space reads one
  when it first selects it, once for every transform after; a set of
  transforms' bound (`Transforms::bytes_bound`) counts them, read or not, as
  it counts the `A2B*` tables.
- **The forward value** a transform out of another profile takes is
  `Profile::connection_value(intent, components)`: the source the intent's
  forward transform converts through, evaluated with no grid between, in
  the profile's own PCS, absolute colorimetric's scaled by the media white
  except through `D2B3`.
- **Matrix/TRC inverted** (RC-20.4): the inverse of the columns, by their
  cofactors over their determinant, computed in `f64` when built and held in
  s15.16, absolute colorimetric's division by the media scale folded into
  its columns; each channel's curve inverted by bisection over its compiled
  table into `CURVE_ENTRIES` entries, each the first table entry at or above
  its light interpolated with the one before it, and read per colour between
  two of them in 16-bit fixed point. A matrix whose determinant is below one
  s15.16 step, or a curve that decreases anywhere or never rises, has no
  inverse, `IccError::NoInverse`. The table step its known answers hold each
  channel to is the span of the curve's exact inverse over the interval of
  inverted entries its light falls in and the one either side of it — the
  matrix's rounding, under one interval, can move a light across an entry —
  and one `u16` unit for the rounding of the entries.
- **Grey inverted**: `Y` (under Lab, `L*` over 100 with `a*`, `b*` ignored)
  through the inverted `kTRC`; under absolute colorimetric `Y` over the
  media scale's, and under Lab the lightness found again from that `Y`.
- **`mft1` and `mft2`** (RC-20.5): `B2A1`, `B2A0`, `B2A2` read into the
  table evaluator with its output count generalised from three to the data
  space's, held to `MAX_DEVICE_CHANNELS` before any of its curves is read
  (`TooLarge`) and then to the data space's count (`UnsupportedSpace`), its
  input count the connection space's three (`MalformedTag`); an `mAB ` at a
  `B2A*` tag is `NeedsLut`, as an `mBA ` at an `A2B*` one is. The PCS is
  encoded on the way in as the table above says, and the table's matrix
  applied, since 10.8 and 10.9 make it the identity unless the input is
  PCSXYZ and a `B2A*` tag's input is the PCS: kept where the input is XYZ
  and the matrix is not the identity, and not read over Lab. The table step
  the known answers hold a channel to is the most a 256th of a grid cell
  along every axis moves the grid's value there, the grid being interpolated
  in 256ths of a cell, carried through the output curve's steepest stretch,
  and four units for the roundings.
- **`mBA `** (RC-20.6): 10.11's order, `B` curves, the matrix with offsets
  (its outputs clipped to `0..1`, on the three channels its `M` curves
  number), `M` curves, the grid, `A` curves, each stage present where its
  offset is not zero, read into the same evaluator with its matrix and `M`
  curves before the grid (`MatrixStage::BeforeGrid`) where an `mAB `'s
  follow it. Its output count is held as an `mft2`'s at a `B2A*` tag, its
  input count is the PCS's three, its `B` curves are required
  (`MalformedTag`), and a tag with no grid has as many outputs as inputs
  (`UnsupportedSpace`); an absent `A` stage is the identity. Its XYZ is
  `u1Fixed15` with no input matrix of `mft2`'s kind, and its Lab the full
  range, as `mAB `'s. `mBA ` at an `A2B*` tag stays `NeedsLut`. The table
  step the known answers hold a channel to is a 256th of the grid's widest
  neighbouring nodes along each axis, and the eight units a coordinate can
  be off by after the curves' and the matrix's roundings, carried through
  the `A` curve's steepest stretch, and four units for the roundings after
  the grid.

##### Device links

A `link` profile (ICC.1 8.6) holds one `AToB0` — `mft1`, `mft2`, `mAB `, or
a `DToB0` — from its data colour space to the space its PCS field names,
which is a device space; the header's rendering intent says which intent
the link was built for and selects nothing. It parses into a link transform
(RC-20.7): device components in, device components out, through the
N-channel evaluator with no connection-space encoding at either end, both
channel counts bounded by `MAX_LUT_INPUTS` and `MAX_DEVICE_CHANNELS`.
`Transform::compile` answers `None` for a link: an `ICCBased` space's
profile may not be one (ISO 32000-2 Table 67), and its output is a
device's, not sRGB, so a page naming one keeps 8.6.5.5's alternate-space
reading. The link is
`icc::Link::apply`, which ED-06b converts documents with.

##### The gamut tag

`gamt` (ICC.1 9.2.28) is a table from the PCS to one channel, in `mft1`,
`mft2` or `mBA `, read beside a profile's output tables. 9.2.28: an output of
zero is in gamut and any other value out. The table interpolates, so a value
is out of gamut where its interpolated output, in the table's own precision,
is half a step or more — a step of 1/255 for an `mft1` or 8-bit `mBA ` grid,
of 1/65 535 otherwise — and in gamut below it (RC-20.8). The answer is
`Output::in_gamut(pcs)`: the value taken as a `B2A*` table takes it, divided
by the media white first under absolute colorimetric, and the table read
with its grid's fractions as they are, in `f64`'s four operations, since the
integer evaluator's 256ths of a cell and whole-step sums are coarser than
the half step the answer turns on. A profile with no `gamt`, or one whose
`gamt` does not read, answers `None`, never in gamut, and its transform
compiles all the same. A proof's gamut warning is RC-05's use of it; nothing
here maps a colour.

##### Named colours

`ncl2` (ICC.1 10.15, Table 63): a vendor flag, the count `n`, the device
coordinate count `m`, a 32-byte prefix and suffix, then `n` entries of
`38 + 2m` bytes — a 32-byte root name, three PCS coordinates in the
encoding of the table above, and `m` device coordinates from `0000h` to
`FFFFh`. Names are 7-bit ASCII, each field NUL-terminated, and a colour's
name is prefix, root and suffix joined; a field holding a byte past 7 bits,
or no NUL, is a malformed tag. `icc::NamedColours` (RC-20.9),
`Profile::named`, is looked up by that name (`NamedColours::get`) and
answers the PCS and device coordinates; sRGB comes through the existing XYZ
and Lab paths (`NamedColour::to_rgb`, `to_rgb16`). A profile of class `nmcl`
(8.9) parses with its colours and no transform, and is refused where its
`ncl2` is absent or does not read, or its PCS is neither `XYZ` nor `Lab`;
any other profile carries its colours beside its transforms, and one whose
`ncl2` does not read carries none. Bounds, before an entry is walked: `m`
within `MAX_DEVICE_CHANNELS`, `n` within `MAX_NAMED_COLOURS`, and
`n × (38 + 2m)` within the tag's own size, both being file fields. The
colours count in `Transforms::bytes_bound`.

##### Version 5

- **The header** (ICC.2 7.2.2, Table 12): the first hundred bytes are
  ICC.1's; bytes 100 to 103 the spectral PCS (Table 21: `rs`, `ts`, `es`,
  `bs` or `sm` and a 16-bit channel count), 104 to 109 its range and 110 to
  115 the bi-spectral range (each a `spectralRange`, 4.2.8: a `float16`
  start and end wavelength and a `u16` step count), 116 to 119 the MCS, 120
  to 123 the sub-class. A spectral-only profile has a PCS field of zero and
  spectral `DToBx` and `BToDx` tags only. Bytes 100 to 127 are read only for
  a version 5 profile; a version 2 or 4 profile's are reserved.
- **Tags** (ICC.2 9.2): `AToB3` and `BToA3` join, ICC-absolute
  colorimetric's own, and absolute colorimetric's row takes `A2B3` after
  `D2B3` and before `A2B1`, unscaled; ICC.2 8.14.2's b) to d) read `AToB0`,
  `AToB1` and `AToB3` in turn after the intent's own, which the rows here
  already reach.
- **Multi-process elements in both directions** (RC-20.10): RC-01.4's reader
  (`icc/mpe.rs`) takes `BToD*` tags as well, the first element's input
  interval the PCS's range — XYZ to what `u1Fixed15` encodes, `L*` 0 to 100,
  `a*` and `b*` −128 to 127 — and a component held there; ICC.2's
  `segmentedCurve` formula types `0003h` to `0007h` (Table 111), its
  `singleSampledCurve` (11.2.2.2) and the extended CLUT (11.2.7), its values
  in any of Table 8's encodings, join ICC.1's elements, and the
  `sampledCalculatorCurve` (11.2.2.3) joins with the calculator (RC-20.11).
  Table 111 gives function type `0006h` a 24-byte field and lists seven
  parameters, all of which its formula reads: seven are read. A version 5
  profile's `AToBx` and `BToAx` tags may hold a `multiProcessElementsType`
  (9.2.1, 9.2.38), read as a `DToBx` tag is; its `DToBx` and `BToDx` tags are
  a spectral connection space's, the `DToBx` reduced below. Which of a multi-process
  tag and an `A2B*` table a profile carrying both converts through is RC-01's
  rule: the `D2B*` tag, 8.10.2 a). Out of the connection space the order is
  the same with `B` for `A` and `D`: the intent's `B2D*` tag first, and
  `B2D3` ICC-absolute as `D2B3` is.
- **The calculator element** (RC-20.11, ICC.2 11.2.1): a main function of
  operations in reverse Polish notation — eight bytes each, a signature and
  four data bytes — over input, output and temporary channels, with
  sub-elements it may invoke. It is validated before it runs (operations
  known, channel indexes in range, the stack never under- or overflowing,
  found by walking the program once), and it runs when the table is built,
  never per colour: the element becomes a grid stage, the program evaluated
  at every node over its inputs' intervals — 33 points an axis up to three
  inputs, 17 for four, 9 for five and six, 5 beyond — the grid's entries
  counted against `MAX_CLUT_ENTRIES`. Every count it can spend is capped,
  and a program past a cap is a typed refusal, `IccError::CalculatorLimit`
  naming it:
  - `MAX_CALC_OPERATIONS` (65 536, the bound `Function::PostScript` holds
    its tokens to): the operations the element and its sub-elements hold,
    each counted once, and the steps one run takes. A step is a value an
    operation takes from the stack or gives to it, one at the least for each
    operation, so a vector operation counts its width; a `solv` adds its
    elimination's multiplications, a conditional counts its longer stream,
    and an invoked sub-element its own steps each time it is invoked — a
    curve set a curve, a matrix a coefficient, a grid a corner and output,
    a calculator its run's and one for the run.
  - `MAX_CALC_STACK` (1 024; ICC.2 reserves 65 535 where a profile states no
    limit): the deepest the stack goes.
  - `MAX_CALC_DEPTH` (16): sub-elements within one another, and conditional
    streams within one another.
  - `MAX_CALC_TEMPORARIES` (1 024): the temporary channels a program
    reaches.
  - `MAX_CALC_WORK` (67 108 864): the steps a tag's calculators take while
    the tag is read — a calculator stage's run at each node of its grid, a
    sampled calculator curve's at each of its entries, each counted as its
    program's steps and one more for the run, before any of them is made.

  A `calculatorLimits` extension (`clmt`) stricter than a cap is honoured,
  for the element's sub-calculators too; its operation count bounds the
  operations they hold, as 11.2.1 states it, and a program within it reads
  however many steps its runs take. Sub-elements naming one position and
  size are one sub-element, and are otherwise apart from one another and
  from the main function (11.2.1); a curve set's channels naming one curve
  share it, its curves otherwise stored one after another (11.2.2.1). Each
  is read once, so a calculator's bytes are read once however its
  sub-elements name them, and every table a sub-element holds — a curve's
  numbers, a matrix's coefficients and offsets, a grid's values — is spent
  from the tag's `MAX_CLUT_ENTRIES` beside its stages' own.
  - **How the program is read.** Table 104 puts an `else` header right after
    its `if`'s, and both before their streams, as Tables 105 and 106 put
    every `case` and `dflt` header before theirs; every stream of a
    conditional leaves the stack as deep as the others, and a `sel` with no
    `dflt` as deep as running no case would. `tint`, `tJab` and `fJab` are
    refused by name (the non-goals above); `env` answers `true` as there and
    one, and every other variable as not there, as Table 92 has a CMM given
    none do. Table 102 prints −1 for a positive value under `sign`, which
    reads +1. The arithmetic is `f64`'s four operations and square root and
    `tinker_pdf_math`'s transcendentals; `asin` and `acos`, which that crate
    does not keep, are its `atan2` of the value against √(1 − x²) and of
    √(1 − x²) against the value.
  - **A sampled calculator curve** (`clcf`, 11.2.2.3) runs its calculator
    at its recommended entry count — held to 4 096, the resolution a curve
    is sampled at after — over its range when the curve is read, and is a
    single sampled curve from then on.
  - **A segmented curve finds its segment by halving its break-points**,
    which never decrease, so a curve of many segments costs a sample or an
    invocation no more than a curve of few.
- **A spectral connection space** (RC-20.12, ICC.2 6.3.3, 7.2.21): a
  version 5 profile whose header names reflectance (`rs`) or transmission
  (`ts`) spectra — its count the steps of the header's spectral range — has
  its `DToBx` tags read as pipelines from the device to the spectrum, one
  channel a wavelength, and reduced to XYZ when built, each spectrum
  integrated against the observer and illuminant the profile's
  `spectralViewingConditionsTag` (`svcn`, ICC.2 9.2.111, of 10.2.22's
  Table 69) carries, both resampled linearly to the spectral range's
  wavelengths and zero outside their own: `X = Σ R(λ) S(λ) x̄(λ) / Σ S(λ)
  ȳ(λ)`, alike for `Y` and `Z`, so a perfect reflector has `Y` of one. The
  reduction is linear, so it is a last matrix stage of three rows of a
  weight a wavelength, built in `f64` and held in s15.16 as every stage is
  (`icc/spectral.rs`). Between the device and the spectrum an element may
  give as many channels as the spectrum has, a matrix's coefficients spent
  from the tag's `MAX_CLUT_ENTRIES` as a grid's are, and the reduction's
  with them; a grid takes no more than `MAX_LUT_INPUTS`, its sixteen counts.
  The spectral tags are the colorimetric connection's last resort: an
  intent converts through them, in ICC.2 8.14.2's order for a spectral
  connection — its own, `DToB0`, `DToB1`, `DToB3` — only where none of the
  profile's colorimetric tags or its model serves it, since 8.14.2 gives a
  colorimetric connection the `AToBx` tags; `DToB3` is ICC-absolute as
  `AToB3` is. A spectral-only profile's connection-space field is zero and
  its spectral tags are all it converts through. Its `BToDx` tags take a
  spectrum, which no colorimetric value gives, and are not read.
- **Viewing conditions** (ICC.2 9.2.111, 10.2.22): a version 5 profile's
  `svcn` describes both its colorimetric and its spectral connection. The
  standard conditions are the CIE 1931 observer under D50, by their types
  (Tables 70 and 71). Under any others the profile connects through its
  `customToStandardPccTag` (`c2sp`, 9.2.57) after every transform into the
  connection space — a table's, a pipeline's, a spectral reduction's —
  and through its `standardToCustomPccTag` (`s2cp`, 9.2.113) before every
  one out of it, its gamut tag's among them, each XYZ in and out; and it is
  refused by name where it carries none, as 10.2.22 has a CMM do, the
  parse refusing a profile with no `c2sp` and the output transform one with
  no `s2cp`. A model under them is passed over, its transform taking no
  stage after it, and refused by name where it alone serves. An `svcn` that
  does not read leaves the colorimetric tags in the standard conditions and
  the spectral ones refused for its reason.
- **What is refused by name** (`IccError::Spectral`): emission,
  bi-spectral and sparse-matrix spectra (the non-goal above); a spectral
  connection space with no `svcn`; a standard observer or illuminant named
  with no steps of its own, or an illuminant its colour temperature alone
  defines (Tables X and Y, the non-goal above); and custom viewing
  conditions with no `c2sp` or `s2cp`. Each refuses the profile only where
  nothing else serves it. Every step count — the spectral range's, an
  observer's, an illuminant's — is held to `MAX_SPECTRAL_STEPS` (1 024; the
  field allows 65 535) before the values it counts are read.

##### Caps, each a `bounds_ledger.rs` row

| Cap | Value | Bounds | Row's step |
| --- | ---: | --- | --- |
| `MAX_DEVICE_CHANNELS` | 15 | a table's output channels, a link's, a named colour's device coordinates | RC-20.16, its document figure the census's most channels a `B2A*` table gives |
| `MAX_NAMED_COLOURS` | 65 536 | an `ncl2` tag's count | RC-20.9 |
| `MAX_MPE_ELEMENTS` | 64 | a multi-process tag's elements (RC-01.4's constant) | RC-20.10 |
| `MAX_CALC_OPERATIONS` | 65 536 | a calculator's operations, its sub-elements' among them, and the steps one run takes | RC-20.11 |
| `MAX_CALC_STACK` | 1 024 | the stack a calculator's program reaches | RC-20.11 |
| `MAX_CALC_DEPTH` | 16 | sub-elements, and conditional streams, within one another | RC-20.11 |
| `MAX_CALC_TEMPORARIES` | 1 024 | temporary channels | RC-20.11 |
| `MAX_CALC_WORK` | 67 108 864 | the steps a tag's calculators take while it is read | RC-20.11 |
| `MAX_SPECTRAL_STEPS` | 1 024 | a spectral range's steps: the connection space's, an observer's, an illuminant's | RC-20.12 |

Fifteen is 7.2.6's `FCLR`, the most channels a data colour space has. Each
row carries the ledger's yardsticks — the most any fixture spends, a comic,
a document, a book, and what the format can reach — and names the test that
fires it before what it bounds is allocated.

##### Integers per colour

Ruling 4's split stands: floats and `tinker_pdf_math` when a table is built,
integers when a colour is converted. An inverted curve is a table, a
multi-process pipeline integer stages, a calculator or a spectral reduction
a grid; only the connection-space conversions at the ends — 6.4's relations,
`lab_to_rgb` — use `tinker_pdf_math`, as the forward pipeline does, which
a forward transform's grid runs once a node rather than once a colour
([P-22](#p-22--colour-conversion-throughput)).

##### Fuzzing

The `icc_profile` target keeps taking a raw profile, with no control prefix
(`xtask/src/fuzzaudit.rs`): it drives the output transform, a link, the
gamut tag and the named-colour lookup with fixed inputs, as it drives every
intent's forward transform now.

##### The census

`icc_census.rs` is a ratchet (RC-20.2, [What the corpus
carries](#what-the-corpus-carries)): `COMPILED` a floor and
`REFUSED_SHAPES` a ceiling, so a parser that reads one more corpus profile
records a gain and one that reads fewer fails; `STREAMS` stays exact, the
lock file's population. The censuses print each refused profile's version,
class, spaces and tags with the structure at each table tag, and count
`B2A*` tags by structure, `D2B*` and `B2D*` tags, `gamt`, `ncl2`, the `link`
and `nmcl` classes, and version 5 profiles with their tag types. RC-20.16
reads them in a local run over the fetched corpora and raises the floor.

#### Milestones

1. RC-20.2: the census ratchet.
   `a_census_that_compiles_more_is_a_gain_and_less_a_failure`.
2. RC-20.4: the output surface; matrix/TRC and grey inverted.
   `icc_known_answers.rs`'s
   `a_matrix_profile_takes_xyz_back_to_its_device_values`,
   `a_grey_profile_takes_luminance_back_through_its_curve` and
   `forward_then_output_is_the_identity_for_a_matrix_profile`.
3. RC-20.5: `B2A*` in `mft1` and `mft2`, and `mft1`'s Lab read in its own
   encoding.
   `a_b2a_lut16_takes_the_connection_space_through_its_matrix_curves_and_grid`,
   `a_b2a_lut8_writes_four_channels_of_ink`,
   `the_relative_colorimetric_b2a_is_chosen_first` and
   `an_mft1_lab_table_reads_in_its_own_encoding`.
4. RC-20.6: `mBA `, and `mAB `'s Lab read in its own encoding.
   `an_mba_tag_runs_its_stages_in_icc_1s_order` and
   `a_v4_table_reads_and_writes_lab_in_its_own_encoding`.
5. RC-20.7: device links. `a_device_link_maps_device_values_through_its_table`
   and `a_link_names_its_output_space_in_the_connection_space_field`.
6. RC-20.8: the gamut tag.
   `a_gamut_tag_answers_in_and_out_where_its_table_says`.
7. RC-20.9: named colours. `a_named_colour_answers_its_coordinates_by_name`
   and `a_named_colour_converts_through_its_connection_space`.
8. RC-20.10: multi-process pipelines both ways and the version 5 header.
   `a_curve_set_matrix_clut_pipeline_is_recomputed` and
   `a_v5_profile_with_a_multi_process_transform_converts`.
9. RC-20.11: the calculator. `the_calculator_computes_what_its_program_says`.
10. RC-20.12: a spectral connection space.
    `a_spectral_profile_integrates_against_its_own_observer`.
11. RC-20.13: the record catches up with the parser: the crate doc, the
    PDF/A notes and these milestones marked.
12. RC-20.14: fingerprints. `determinism.rs`'s
    `icc_transforms_are_stable_across_targets` and
    `a_v5_profiled_page_renders_the_same_on_every_target`.
13. RC-20.15a: the speed row `an ICC-based image to sRGB through a v5
    multi-process profile`; RC-20.15b: its wall-clock figure.
14. RC-20.16: the corpus figure, ratcheted.

#### Risks

| Risk | Mitigation |
| --- | --- |
| An inverted curve or matrix is ill-conditioned and the output transform amplifies a rounding into a visible error | The inverse is built in `f64` and held to s15.16; a determinant under one step or a decreasing curve is `NoInverse`; `forward_then_output_is_the_identity_for_a_matrix_profile` holds the round trip within a table step |
| The Lab correction for `mft1` and `mAB ` moves pages that render today | No pinned page paints through a Lab table of either type; the corpus pages that do move, each Lab component by at most 0.4 % of its range, toward what 10.9 and 10.10 say, and RC-20.16's run counts them |
| A calculator program costs without bound | Every count it can spend is capped and checked before it runs, the steps a run takes and the steps all of a tag's runs take among them, and it runs only when a table is built, never per colour, the precedent `Function::PostScript` set |
| A spectral profile reduced against a guessed observer | Only the observer and illuminant the profile carries are used; a standard observer named without its table is refused by name, never assumed |
| v5's precedence differs from ICC.1's for a profile carrying `AToB3` | Absolute colorimetric's row reads `A2B3` before `A2B1` for a version 5 profile only; every version 2 and 4 profile keeps RC-01's rows |
| An element ICC.2 defines is read wrongly because no published table settles its arithmetic | Each element's known-answer test recomputes it from the clause; an element this design does not read is refused by name, so its tag is passed over rather than half-read |

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
20. RC-01.9: a profile carrying a model beside `A2B*` tables converts
    through its tables first, as 8.10.2 orders them, and one whose model
    contradicts its header through its tables.

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
| Compiling per intent multiplies what a profile costs | A table is read and compiled on first use, one transform a distinct source; a page that sets no intent reads and compiles only what relative colorimetric converts through |
| A profile carrying a matrix or `grayTRC` and `A2B*` tables that disagree, whose pages render through the tables where its model would paint otherwise | ICC.1 8.10.2's order, which the owner took (ruling 23, RC-01.9); `a_profile_carrying_a_model_and_tables_reads_its_tables_first` and `a_table_that_does_not_read_is_passed_over_to_the_model` pin each order case, and RC-01.9b counts the corpus profiles that carry both |

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
  evaluation and the tests' reference, `Transform::evaluate`, the pipeline
  itself.
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

P-22.6 chooses one converter an image, by its space, depth and `/Decode` and
whether a colour key reads it, in the sample loops of a stream, an inline
image, a JPEG's strips and a JPEG 2000 image's samples (`resources.rs`'s
`Converter`), each arm asked in this order:

| Image | Converter |
| --- | --- |
| 8-bit DeviceRGB or DeviceGray, or a refused profile approximated as either, `/Decode` absent or `[0 1]`, no colour key | a copy, a grey replicated to three (P-08's identity path) |
| 8-bit ICC, `/Decode` absent or `[0 1]`, no colour key, a transform over the space's components | the row path |
| Any one-component space at up to 8 bits, DeviceGray among them, any `/Decode` | a table of 2^bpc colours, an entry filled through `to_rgb` the first time a sample holds its value |
| 8-bit DeviceCMYK, or a profile approximated as it, `/Decode` absent or `[0 1]`, and a CMYK JPEG in its own colour | one table of 8.6.4.4's `(1 − x)(1 − k)`, 256 × 256 bytes read for each of C, M and Y against K, an entry filled by the `to_rgb` of the pixel that first asks for it, which fills its three at once |
| Everything else | `to_rgb` over buffers the image keeps — its components, and a tint transform's outputs (`ColorSpace::to_rgb_with_in`) — with `Remembered` |

A table filled through `to_rgb` is each device pixel's byte by construction —
DeviceCMYK's channels are each one relation of their ink and black, so an
entry is that relation's byte whichever pixel filled it — and the row path is
`apply`'s, so the converter moves no pixel. Filled as the pixels first ask, a
table costs an image no more conversions than its pixels and none more than
its entries, so a small image pays nothing for a table it does not read.
`Stats::image_colours_converted` counts what each converter asks.

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
- **The bytes each stream holds** are kept beside the entries, under the
  stream's object (`cache::ProfileKey`) and charged their length: a scope's
  question about an `ICCBased` space names an object, and reading what its
  bytes are once a document, rather than decoding and digesting the stream at
  every question, keeps a kept profile's every later question free of both. A
  stream that will not decode has no bytes, and is refused under its object.
- **A refusal is kept** under the profile's bytes, so a profile that does not
  parse is refused once a document, and a stream that will not decode under
  its object, likewise once.
- **The charge**, taken when the entry is inserted: the parsed profile's
  tables — its `A2B*` and `D2B*` elements as `Profile::parse` holds them,
  or a matrix/TRC transform's three 4 096-entry curves — and, for each
  distinct gridded transform its conversions can compile — a table or a
  `D2B*` tag's elements, and absolute colorimetric's scaling of a table where
  the media white is not the PCS white — a grid's whole node array at eight bytes a node, with P-16.1's 64-byte
  floor. That is the most the entry can come to
  hold, charged before any node is filled, so filling never passes the
  budget. An entry the budget cannot hold is used by the request that made it
  and not kept, so past the budget a transform compiles per use, as today.

#### Hostile profiles (ruling 1)

A grid exists only over one, three or four inputs, and a table or a `D2B*`
tag over any other count converts as today. A node's evaluation is the
pipeline once: a table's, whose work is bounded by its input count — at most
sixteen corners over four inputs — and not by how large the profile's own
table is; a `D2B*` tag's, by what RC-01.4 holds a tag to compile —
`MAX_MPE_ELEMENTS` elements, sixteen channels and `MAX_CLUT_ENTRIES` grid
entries in all — the work each colour paid before the grid stood in for it.
So a grid's whole build is at most 83 521 evaluations and its memory at most
668 168 bytes whatever the profile declares, under a third of `MAX_ICC_BYTES`, the most a profile itself
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

## Soft proofing (RC-05)

*For [ROADMAP](../ROADMAP.md) row RC-05.* A soft proof renders a page as a
given output device would reproduce it, and shows that reproduction on the
screen: each colour is taken into the device's colorants, the page composites
there, and the result is taken back to the display. ISO 32000-2 names both
halves. 14.11.5's `/DestOutputProfile` is "an ICC profile stream defining the
transformation from the PDF document's source colours to output device
colourants … The output transformation uses the profile's "from CIE"
information (BToA in ICC terminology); the "to CIE" (AToB) information may
optionally be used to remap source colour values to some other destination
colour space, such as for screen preview or hardcopy proofing", and 10.1
makes the device the clause renders for one that may be "just assumed (for
example to simulate the colour appearance of PDF output on one raster output
device by using some other raster output device)".

### Scope

- **One composition.** The proof transform of a colour is the display space,
  from the proof profile's input direction, from its output direction, from
  the colour's own space: `V(D(c))`, where the **device leg** `D` takes a
  colour into the proof device's components and the **display leg** `V`
  takes those components to the display.
- **Where each leg runs**: `D` where a colour is read, at the resource seam,
  and wherever the engine turns light into the device's values; the page
  composites in the proof device's space; `V` per pixel on the finished page.
- **Two intents and a paper**: `D` at the intent in force for the colour,
  `V` relative colorimetric unless the caller asks for the paper.
- **Two sources**: a profile the caller hands over, and the document's own
  output intent, chosen by the rule below.
- **The integer form**, its tables and the bound the tests hold it to.
- **Its composition with RC-06's output pixel models**, and its refusals.

### Non-goals

- **A gamut warning.** RC-20.8 reads the `gamt` tag and answers in or out of
  gamut; marking out-of-gamut pixels on a proof is no step of RC-05.
- **Spot inks through an output intent's PDF 2.0 data** — `/MixingHints`,
  `/SpectralData`, a `/DestOutputProfileRef`'s colorant table. 14.11.5 says
  of the first two that "This document intentionally does not specify how a
  PDF processor may make use of the data … and a PDF processor may ignore
  such data altogether". A `/Separation` or `/DeviceN` reaches the proof
  device through its alternate, as below.
- **A profile by reference.** `/DestOutputProfileRef` names profiles by
  checksum, URL or embedded file (Table 402); only the embedded
  `/DestOutputProfile` stream is read.
- **A standard production condition with no profile.** 14.11.5 lets an
  intent name a registered condition by `/OutputConditionIdentifier` alone,
  "the production software … responsible for providing the corresponding
  ICC profile"; no registry's profiles are shipped, so such an intent proofs
  nothing and says so.
- **Proof profiles of more than four channels, device links, abstract and
  named-colour profiles** (`link`, `abst`, `nmcl`): refused by name, below.
- **Black-point compensation on either leg** is RC-02's parameter, which both
  legs take when it lands; **overprint simulation** is RC-03's switch, drawn
  on the same ink canvas a CMYK proof uses.

### Design

#### What a colour is on the proof device

10.3.2 is the rule: "A PDF processor should establish CIE-based colour
specifications for device colour spaces … when those device colour spaces do
not match that of the raster output device. If the native device colour space
is CMYK, then converting colours in the DeviceGray colour space to that CMYK
should follow the method described in 10.4.2.3". So a device colour of the
proof device's own family **is** the device's value, and every other colour
is taken through the connection space. The engine's CIE-based reading of a
device space is the one it renders the screen by: DeviceRGB as sRGB
(`SRGB_TO_XYZ_D50`), DeviceGray as an sRGB grey, DeviceCMYK as its 8.6.4.4
light read as sRGB.

| The colour's space | Grey proof | RGB proof | CMYK proof |
| --- | --- | --- | --- |
| DeviceGray | its own grey | sRGB grey, through `D` | `0 0 0 1−g`, 10.4.2.3 |
| DeviceRGB | sRGB, through `D` | its own components | sRGB, through `D` |
| DeviceCMYK | its light, through `D` | its light, through `D` | its own components |
| `ICCBased`, `CalGray`, `CalRGB`, `/Lab`, a default space (8.6.5.6) | its own connection-space value through the output direction | the same | the same |
| `/Indexed` | its base's colour, by this table | the same | the same |
| `/Separation`, `/DeviceN` | its alternate's colour, by this table | the same | the same, or its plates where RC-04 composites spots |
| An uncoloured pattern's colour | its base's colour, by this table | the same | the same |

**A DeviceCMYK colour under a CMYK proof is the proof device's own ink**: its
components composite on the ink canvas as they are (`GraphicsState::fill_ink`,
the slot RC-04.6 generalises to colorants), and only `V` converts them:
10.3.2 remaps a device space only where it does not match the device's, so on
a CMYK device DeviceCMYK is the device's own colorants. Under a `/DefaultCMYK`
it is H-135's answer for an ink render too: the document's own components. Its light, which a buffer of another family reads, is `V` of
those components, since the proof profile is what characterises DeviceCMYK on
this device; the same holds for a DeviceGray colour under a CMYK proof, whose
ink is 10.4.2.3's.

A colour's own connection-space value is its profile's `A2B*` at the intent in
force (RC-01), and a `CalGray`, `CalRGB` or `/Lab` colour's is its closed form
(8.6.5.2–8.6.5.4), which `parse_space` already reads; so a wide-gamut colour
reaches the proof device without being clipped to sRGB first. That is why `D`
runs where the colour is read and not on a composited sRGB page.

#### Where each leg runs

1. **The page composites in the proof device's space.** The page canvas is
   `Gray8`/`GrayA8` for a grey proof, `Rgb8`/`Rgba8` for an RGB one, and
   RC-04's ink canvas — its four process plates, `CmykA8`'s layout — for a
   CMYK one, and its values are the device's.
2. **A colour arrives as its device value, computed at the resource seam**
   from its own space by the table above: on a grey or RGB proof in the light
   slot (`fill_color`), so that every buffer of the render holds the device's
   light; on a CMYK proof in the ink slot, beside a light that is `V` of it.
   The seam is asked for every colour — the interpreter's device operators
   take the path they take under a default space (`FontSource::device_defaults`
   answers that all three are remapped), so `g`, `rg` and `k` resolve through
   `resolve_color` and `resolve_ink` like `sc`. Images decode to device values
   at the seam, through the decoded-image cache keyed by the proof as it is by
   the intent (RC-01.6); on a CMYK proof by RC-04.12's samples kept for plates.
   Shadings and meshes are evaluated in the renderer (`shading.rs`, `mesh.rs`),
   which is handed the compiled proof — a PDF-free value of `tinker-pdf-color` —
   and converts each colour they evaluate through it, on a CMYK proof by
   RC-04.13's and RC-04.14's colorant paths.
3. **Light turned into ink on a CMYK proof goes through `D`**, not through
   maximum undercolour removal: a group of another family composited onto an
   ink buffer (11.6.6's boundary conversion), and a page whose `/Group` names
   another family (11.4.7: the page group is composited and then converted to
   the device). The ink canvas carries its light-to-ink conversion as a value
   — the seam RC-08.2 gives black generation and undercolour removal — and a
   proof sets it to `D` from sRGB light. A non-isolated group of another
   family over device values starts from them through `V` (11.4.4).
4. **On a grey or RGB proof a group of another family** — `/DeviceCMYK`,
   `/Lab` — composites as it does today, its boundary conversions 8.6.4.4's
   and Lab's relations applied to the device's values as light. They round
   trip exactly (the CMYK relation's inverse is exact, [Stage 1](#stage-1-transparency-group-colour-spaces)),
   so `Normal` compositing inside such a group is exact; a blend mode inside
   it blends the device's values in that family's encoding, a reading this
   design states rather than a conversion it performs.
5. **`V` runs per pixel on the finished page canvas**, at the end of
   `render_layer_with`, before any output conversion RC-06 makes. It reads the
   pixel alone, so a tile's pixels are the page's there (ruling 5), and alpha
   is kept.

#### Intents and the paper

- **`D` takes the intent in force for the colour** — `ri`, `/RI` and an
  image's `/Intent` as RC-01 selects it — through the output direction's
  table for that intent (`B2A0`, `B2A1`, `B2A2`; absolute colorimetric
  through `B2A1` scaled by the media white, RC-01.3), with RC-01's fallback
  where a table is absent. The output direction is compiled per intent as
  [What compiling per intent costs](#what-compiling-per-intent-costs)
  compiles the input direction: each distinct `B2A*` table the first time a
  colour asks for it, absolute colorimetric's scaling applied after the
  relative table's, so a proof holds at most one output transform a distinct
  table.
- **`V` is relative colorimetric**, so the paper the device prints on shows
  as the display's white; **with the paper asked for, absolute colorimetric**,
  the profile's media white scaled against D50 as RC-01.3 scales it, so a
  newsprint condition shows its paper's colour. One display intent per
  render, chosen with the proof; the page's own intents do not reach it,
  since the display is not the content's destination.
- **The page's background** is the device's own paper — no ink on a CMYK
  proof, `1` on a grey one, `1 1 1` on an RGB one — and `V` shows it as
  white, or as the paper.

#### Which output intent proofs a page

14.11.5 settles the page against the catalog and leaves the subtype to the
workflow: "The choice of which output intent to use in a given workflow is
outside the scope of ISO 32000-2 and therefore PDF intentionally does not
include a selector for choosing a particular output intent from within the
PDF file", and "If a PDF processor chooses to respect output intents, then
when processing a page that has an associated (page-level) output intent,
that page-level output intent shall be used." So the caller names the
workflow's subtype — `GTS_PDFX`, `GTS_PDFA1`, `ISO_PDFE1` or another — or
none, and:

1. The **candidates** of a list are its entries of that subtype, in array
   order; every entry when none is named.
2. The page's own `/OutputIntents` (PDF 2.0, `Page::output_intents`) are
   read first: **if they hold a candidate, the page's list is the one used**
   and the catalog's is not read. A page whose intents are all of other
   subtypes has no page-level intent for this workflow, and the catalog's list
   (`Document::output_intents`) is used.
3. In the list used, the **first candidate carrying a `/DestOutputProfile`**
   proofs the page.
4. A list whose candidates carry no profile, and a document with no
   candidate at all, render the page unproofed and name it (ruling 2 and
   10): `RenderWarning::ProofUnavailable`, with the intent's
   `/OutputConditionIdentifier` where there is one and a reason —
   `NoOutputIntent`, `NoDestinationProfile`, or `Refused` with the
   `ProofError` its profile met. A refused profile is not passed over for the
   next candidate: the next is another output condition.

The choice is one function in `crates/tinker-pdf/src/output_intents.rs`.
SD-16.6 implements how a page's intents combine with the catalog's for every
reader from the same clause; if it reads the clause otherwise, the proof's
choice moves with it in that one function.

#### The proof's integer form, its tables, and the bound

Ruling 4: no float and no transcendental per colour. The proof is
`icc::Proof`, compiled once from profile bytes with its display intent, and
each stage is a table or a fixed-point product built when it is compiled:

| Stage | Integer form | Table | Why that size |
| --- | --- | --- | --- |
| sRGB light in (DeviceRGB, the grey and CMYK readings, light at a boundary) | IEC 61966-2-1's decode, s15.16 | 256 per channel | the domain is a byte: the table is the function at every input |
| to the connection space | `SRGB_TO_XYZ_D50`, or the source profile's `A2B*` as RC-01 compiles it per intent | — | the source's own form |
| the output direction | RC-20's `icc::Output` at the colour's intent: the inverted matrix in s15.16 and inverted curves, or a `B2A*` table | RC-20.1's | RC-20's own tables, not resampled |
| device components | rounded to bytes | — | the canvas holds bytes |
| `V`, a grey proof | the profile's grey to sRGB | 256 | the domain is a byte: exact |
| `V`, an RGB matrix/TRC proof | per channel the curve to linear s15.16, the profile's columns times XYZ-to-sRGB in s15.16 as `Transform::compile` builds it | 256 per channel | the domain is a byte |
| `V`, a lookup-table proof | the profile's `A2B*` in integers: input curves, the grid multilinear with 16-bit fractions at the profile's own nodes, output curves interpolated in 16 bits, a Lab connection space through XYZ with the cube in fixed point | the profile's own grid | resampling a grid adds an interpolation error on top of the one its author chose |
| `V`'s sRGB encode | IEC 61966-2-1's encode, the index rounded | 2^14 + 1 | `12.92 × 255 / (2 × 16 384)` is 0.10 of a level: under an eighth at the steepest point of the curve |
| `V` for three and four channels | a memo of 4 096 slots keyed by the pixel's bytes | — | it holds what the evaluation returns, so it changes no byte; the `/DeviceN` image path's table is the precedent |

The encode table is the proof's own: the screen path's `encode_srgb`
(4 096 entries, its index truncated, worth up to 0.8 of a level near black)
keeps its bytes.

**The bound is derived per sample, from the tables above.** The tests'
reference is ICC.1's equations in `f64` — curves and matrices in closed form,
a grid at its nodes and, between them, by the test's own multilinear
interpolation, which pins this engine's interpolation (ruling 13's named
limit for tables). Each stage quantises by half a step of its table where it
rounds and a whole step where it truncates; the error a stage leaves is
carried to the output through the slopes of the stages after it, at that
sample. Summed over the stages and in levels of 255, that is `E`, and a
sample is held to `|byte − 255 × exact| ≤ ½ + E`. The test computes `E` for
the profiles it builds — their closed forms give their slopes — so the bound
is a figure the test derives rather than one chosen to pass. For the matrix
and grey profiles RC-05.2 builds, `E` stays under half a level away from
black and the bound is one level of 255; near black an inverted gamma curve's
slope makes `E`, and the bound there, larger, which is the derivation working
rather than a tolerance widened. A proofed page's sample in RC-05.3 composes
the two legs: the device byte the test computes, then `V` of it, each leg
within its `E`, `V`'s carrying `D`'s error through `V`'s slope.

#### With RC-06's output pixel models

`V`'s display space is sRGB, or RC-06.5's destination when the render has
one: then `V` is the proof's input direction composed with the destination's
output direction, through the connection space once, compiled once per proof
and destination, rather than `V` to sRGB and the destination after it, which
would clip the proof to sRGB. The BGR orders (RC-06.2) reorder `V`'s result;
a 1-bit render (RC-06.3, RC-06.4) screens the grey of `V`'s result.

#### Refused by name

`Proof::new(bytes, display)` is built once and returns `Result<Proof,
ProofError>` (ruling 2: the caller hears why at once, rather than every page
rendering unproofed):

| `ProofError` | When |
| --- | --- |
| `Profile(IccError)` | `Profile::parse` refuses the bytes |
| `NoOutputDirection` | no `B2A*` table and no matrix or curve the output direction can invert — an input profile carrying `A2B*` alone, a matrix with no inverse |
| `NoInputDirection` | no `A2B*` table and no matrix: nothing takes the device to the display |
| `DataSpace([u8; 4])` | a data space other than `GRAY`, `RGB ` and `CMYK`: no canvas holds its components |
| `Class([u8; 4])` | a device link, an abstract or a named-colour profile, which has no device side to proof |

A document's intent whose profile is refused is the `Refused` warning above;
`MAX_ICC_BYTES` bounds a document's profile as it bounds any.

### Milestones

1. **RC-05.2, the proof compiled** in `tinker-pdf-color`. Exit: `icc.rs`'s
   `a_proof_transform_is_its_legs_composed` — a matrix/TRC RGB, a grey and a
   LUT CMYK proof profile built in the test, each intent, at the tables'
   nodes and between them, within the derived bound; each `ProofError`
   reached by a profile built for it; `cargo xtask libm` passes; the
   `icc_profile` fuzz target compiles and applies a proof.
2. **RC-05.3, a caller's profile renders**: `RenderOptions` carries the
   compiled proof, and `Page::render`, `render_form`, `render_annotation` and
   a display list's render proof alike. Exit: `soft_proof.rs`'s
   `a_proofed_page_shows_its_colours_through_the_proof_transform` — fills in
   DeviceRGB, DeviceGray, DeviceCMYK and an `ICCBased` space through a
   matrix/TRC and a LUT CMYK profile, each sample the composition the test
   computes within the bound, a DeviceCMYK fill under the CMYK proof its own
   components through `V` and a DeviceGray fill 10.4.2.3's, a `/DeviceRGB`
   group under the CMYK proof taken through `D` at its close, the paper shown
   under absolute colorimetric; the same page unproofed byte-identical to
   today's render; `render_regions.rs`'s `proofed_tiles_are_the_proofed_page`.
3. **RC-05.4, the document's output intent.** Exit: `soft_proof.rs`'s
   `a_page_proofed_through_its_output_intent_is_proofed_through_that_profile`
   — a `GTS_PDFX` intent's page byte-identical to its twin proofed through
   the same profile by the caller; a PDF 2.0 page's own candidate chosen over
   the catalog's, and the catalog's chosen for a page whose intents are all of
   another subtype; the first candidate with a profile chosen over one
   without; no candidate, no profile and a refused profile each rendering the
   page unproofed with its `ProofUnavailable`; the `render_page` fuzz target
   proofs a page through its own intent.
4. **RC-05.5, fingerprints**: `soft_proof.rs`'s
   `proofed_pages_are_stable_across_targets` on four targets, entered in
   K-07's roster.
5. **RC-05.6a and RC-05.6b, the speed row**: a page of mixed fills and an
   image at 150 dpi proofed through a LUT CMYK profile beside the same page
   unproofed, in MS-04's render group; RC-05.6b takes its figure.

### Risks

| Risk | Mitigation |
| --- | --- |
| No second colour engine says whether a proof is right (ruling 13) | Every stage is held to ICC.1's equations at a bound the test derives from the tables; between a grid's nodes the test pins this engine's interpolation, the limit [Verification](#verification) already names |
| `V` per pixel through a four-channel table is slow | The memo makes a page of flat colours cost a lookup a pixel; RC-05.6 measures the page that is not |
| A colour reaches the device by two paths that disagree — through the seam, and as light through `D` at a group's close | Both are `D`, from the colour's own space where it is read and from its light where only light is left; a proofed page's tests include a group of another family on each proof |
| A CMYK proof needs RC-04's plates, and a grey or RGB proof only today's canvases | RC-05.3 depends on RC-04.10; the grey and RGB proofs reuse the canvases as they are |
| The proof slows every render, proofed or not | Off by default and absent from every unproofed path: an unproofed page is byte-identical, and the seam's added question is asked only when a proof is in force |
| A page-level intent read the other way by SD-16.6 | The choice is one function, which SD-16.6 moves if it reads 14.11.5 otherwise |
| A hostile output intent profile | `MAX_ICC_BYTES`, the parser's refusals and the `icc_profile` and `render_page` fuzz targets, as for any profile |

## A first-party sRGB profile (RC-19)

*For [ROADMAP](../ROADMAP.md) row RC-19.* The engine converts every colour to
sRGB through the matrix `XYZ_D50_TO_SRGB` and IEC 61966-2-1's encode
function, which are not a profile a caller could embed. The two places a
document wants an RGB profile to name are each a profile the caller states:

- **The default RGB space.** `DeviceRGB` is drawn as sRGB, and a scope's
  `/DefaultRGB` (8.6.5.6) is the document's own `ICCBased` or CIE space
  ([Default colour spaces](#default-colour-spaces-8656)); a caller writing a
  document whose RGB it wants characterised names a profile there.
- **PDF/A conversion's default output intent.** No conversion exists yet.
  The writer's archival profile (`ArchivalProfile::destination_profile`,
  [pdfa](pdfa.md)) takes the destination profile as a mandatory statement of
  the caller's, and SD-02's conversion — "device colour converted or an
  output intent added" — is the first user that would add one unasked, with
  ED-06b's document conversion to sRGB, RC-05's soft proofing and RC-20's
  output direction after it, each naming sRGB as a profile.

This section specifies the profile the engine computes for all of them: its
header, its tags and their types, the numbers it is computed from and their
source, and the surface that hands it out. The ICC's own published profile
files stay refused (NG-03): this profile is computed, not vendored, so
`cargo xtask vendor` has nothing to admit and THIRDPARTY.md nothing to list.

### Scope

- **The bytes**: one ICC profile, version 2.4.0, class `mntr`, data space
  `RGB `, connection space `XYZ `, written by `crates/tinker-pdf-color/build.rs`
  into `OUT_DIR` from IEC 61966-2-1's published primaries, white point and
  transfer function.
- **The surface**: `tinker_pdf_color::icc::SRGB_PROFILE`, a `&'static [u8]`
  of those bytes, projected by the facade as `tinker_pdf::SRGB_PROFILE`
  (ruling 11), which a caller states as an archival destination profile or
  as the stream of an `/ICCBased` `/DefaultRGB`.
- **One byte string on every build host**, pinned by a fingerprint.

### Non-goals

- **Bundling the ICC's own files** (NG-03): they carry no SPDX identifier.
  Nothing here reads, copies or ports another profile's bytes; every number
  is computed from the published values below.
- **A default.** `ArchivalProfile::destination_profile` stays mandatory and
  `DeviceRGB` under a CMYK destination stays refused: the device a
  document's colours are for remains the caller's statement
  ([pdfa](pdfa.md)), now with a first-party profile the caller may state.
  Nothing an unstated document renders, writes or validates changes.
- **A version 4 profile.** `parametricCurveType`, `multiLocalizedUnicodeType`
  and the profile ID are version 4's, and ISO 19005-1 reads ICC.1:1998-09,
  under which `pdfa/colour.rs`'s `header_defects` admits a version below 3.0
  only; a version 2 profile is admitted under every part.
- **Grey and CMYK profiles.** A grey profile's curve is not a standard's
  number, and a CMYK characterisation is a press's, which is the caller's to
  state.
- **Reading `chad`.** The profile carries one; the CMM does not read it
  ([Three decisions](#three-decisions-and-why)), and needs not: the colorants
  are already adapted.

### Design

The clauses below are ICC.1:2001-04's, the edition that defines profile
version 2.4.0 (6.1.3). Every field and tag is big-endian (clause 6).

#### The header (6.1, Table 9)

| Bytes | Field | Value | Clause |
| --- | --- | --- | --- |
| 0..3 | Profile size | the length of the whole profile | 6.1.1 |
| 4..7 | CMM type | 0: no CMM preferred | 6.1.2 |
| 8..11 | Profile version | `02400000h`, 2.4.0 | 6.1.3, Table 10 |
| 12..15 | Device class | `mntr`, a display device profile | 6.1.4, Table 11 |
| 16..19 | Data colour space | `RGB ` | 6.1.5, Table 13 |
| 20..23 | Profile connection space | `XYZ ` | 6.1.6, Table 14 |
| 24..35 | Date and time created | a fixed `dateTimeNumber` (5.3.1) written in `build.rs`, so the bytes are a constant | Table 9 |
| 36..39 | File signature | `acsp` | Table 9 |
| 40..43 | Primary platform | 0: none | 6.1.7 |
| 44..47 | Flags | 0: not embedded, usable on its own | 6.1.8, Table 16 |
| 48..55 | Device manufacturer and model | 0: no registered signature | 6.1.9 |
| 56..63 | Attributes | 0 | 6.1.10, Table 17 |
| 64..67 | Rendering intent | 0, perceptual | 6.1.11, Table 18 |
| 68..79 | PCS illuminant | D50, `0000F6D6h 00010000h 0000D32Dh` | Table 9, A.1 |
| 80..83 | Profile creator | 0: no registered signature | 6.1.12 |
| 84..127 | Reserved | 0 | Table 9 |

A.1 sets the PCS illuminant to D50, `[X=0.9642, Y=1.0000, Z=0.8249]`. Its
encoding is the one `tinker_pdf_color::icc::PCS_WHITE` reads back as exactly
the PCS white, so the media white point below scales this engine's
ICC-absolute colorimetry by exactly one.

#### The tag table (6.2) and the tags

The table (Table 19) lists ten tags in this order, each element's offset on a
four-byte boundary (6.2.2) and its size the data's own, with no padding
(6.2.3); the data follow the table in the same order, each padded with zero
bytes to the next boundary. The three tone curves are one element that the
three tags name, which 6.3 allows ("tags may reference the same tag data").

| Tag | Clause | Type | Contents |
| --- | --- | --- | --- |
| `desc`, profileDescriptionTag | 6.4.32 | `textDescriptionType`, 6.5.17, Table 68 | the invariant ASCII description `sRGB IEC 61966-2-1 (tinker-pdf)` and its null; Unicode language code and count 0, ScriptCode code and count 0 and the 67-byte Macintosh description zero, as 6.5.17 asks where neither is localised |
| `cprt`, copyrightTag | 6.4.13 | `textType`, 6.5.18, Table 70 | `Copyright tinker-pdf contributors, MIT OR Apache-2.0` and its null |
| `wtpt`, mediaWhitePointTag | 6.4.25 | `XYZType`, 6.5.26, Table 78 | the PCS white, encoded as the header's illuminant |
| `chad`, chromaticAdaptationTag | 6.4.11 | `s15Fixed16ArrayType`, 6.5.14, Table 63 | the adaptation matrix below, nine `s15Fixed16Number`s by rows |
| `rXYZ`, `gXYZ`, `bXYZ`, the colorant tags | 6.4.40, 6.4.20, 6.4.4 | `XYZType` | the adapted columns below |
| `rTRC`, `gTRC`, `bTRC`, the tone curves | 6.4.41, 6.4.21, 6.4.5 | `curveType`, 6.5.3, Table 37 | 1 024 entries of the transfer function below |

These are Table 25's required tags of an RGB display profile (6.3.2.2) —
description, the three colorants, the three curves, the media white and the
copyright — and `chad`, which Annex E asks every profile to carry ("profile
applications should include this tag to improve interoperability"). Every
type is version 2's.

**The media white is the PCS white.** A.3.1.1: "if the viewer completely
adapts to the white point of the medium (as is often the case with monitors)
this tag should be set to Xi, Yi, Zi (the PCS white point)", and the
adaptation applied to the colorants is applied to it as well, which takes
D65 to D50. So in this engine ICC-absolute colorimetry through the profile
is media-relative colorimetry, bit for bit, and a CMM that reads `chad`
recovers D65 as the display's white.

#### The numbers, and where they are published

IEC 61966-2-1:1999's values are taken from a free publication of the final
standard's, W3C's CSS Color Module Level 4: section 10.2 ("The Predefined
sRGB Color Space") for the primaries and the transfer function, and section
2 for the white point. The standard's threshold is 0.04045 — the 1996
proposal's 0.03928 is the draft's — which is the one the `para` fixtures in
`icc.rs` already state, beside `encode_srgb`'s 0.003 130 8 on the linear
side.

| Quantity | Value | Source |
| --- | --- | --- |
| Red, green, blue chromaticities `(x, y)` | (0.640, 0.330), (0.300, 0.600), (0.150, 0.060) | CSS Color 4, 10.2 |
| White point, D65 `(x, y)` | (0.3127, 0.3290) | CSS Color 4, 10.2 and 2 |
| Transfer function, encoded `c` to linear | `c / 12.92` for `c ≤ 0.04045`, else `((c + 0.055) / 1.055)^2.4` | CSS Color 4, 10.2 |
| PCS white, D50 `XYZ` | (0.9642, 1.0000, 0.8249) | ICC.1:2001-04 A.1 |
| Bradford cone matrix, by rows | (0.8951, 0.2664, −0.1614), (−0.7502, 1.7135, 0.0367), (0.0389, −0.0685, 1.0296) | ICC.1:2001-04 E.1.2 (E.6) |

**The colorant columns.** Each chromaticity `(x, y)` is the `XYZ` value
`(x / y, 1, (1 − x − y) / y)`. With the three primaries as the columns of
`P`, the scales `S = P⁻¹ W` make `M = P · diag(S)` the matrix that takes
linear RGB to `XYZ` relative to D65, white to `W`. The adaptation to the
connection space is E.1.2's linearized Bradford transform (E.7): the cone
responses of the source white `ργβ_src = M_BFD W` and of the PCS white
`ργβ_pcs = M_BFD W_D50`, and
`A = M_BFD⁻¹ · diag(ρ_pcs / ρ_src, γ_pcs / γ_src, β_pcs / β_src) · M_BFD`.
The colorants are the columns of `A · M`, each component rounded to the
nearest `s15Fixed16Number` (5.3.3), and `A` is what `chad` holds, rounded the
same way. Computed in exact arithmetic from the values above, they are:

| Tag | `X` | `Y` | `Z` |
| --- | --- | --- | --- |
| `rXYZ` | `00006FA0h` (0.436 041) | `000038F5h` (0.222 485) | `00000390h` (0.013 920) |
| `gXYZ` | `00006297h` (0.385 113) | `0000B787h` (0.716 905) | `000018D9h` (0.097 067) |
| `bXYZ` | `0000249Fh` (0.143 046) | `00000F84h` (0.060 610) | `0000B6C3h` (0.713 913) |

and `chad`, by rows, `00010C42h 000005DEh FFFFF325h`,
`00000793h 0000FD90h FFFFFBA1h`, `FFFFFDA2h 000003DCh 0000C06Eh`
(1.047 886, 0.022 919, −0.050 216; 0.029 582, 0.990 484, −0.017 079;
−0.009 252, 0.015 073, 0.751 678). The columns sum to the PCS white before
rounding, since `A` takes `W` to it exactly.

**The tone curve.** Entry `i` of 1 024 is the transfer function at
`c = i / 1023`, as a `uInt16Number` scaled so that 65 535 is 1.0 (6.5.3) and
rounded to the nearest: entry 0 is 0, entry 1 023 is 65 535, the linear
segment ends between entries 41 and 42, and the engine reads the curve by
linear interpolation, as it reads every sampled curve. 1 024 entries put the
interpolation's error under a thousandth of an 8-bit level.

#### `build.rs`, and one byte string on every host

`crates/tinker-pdf-color/build.rs` computes all of it at build time and
writes `srgb.icc` into `OUT_DIR`; `icc.rs` takes it with `include_bytes!`.
It reads no file and fetches nothing. Its arithmetic is IEEE 754's four
operations, which every target rounds alike, and its one transcendental, the
curve's power, is `tinker_pdf_math::pow`, taken as a build-dependency (the
`color → math` edge already exists): so every build host writes the same
bytes, whatever the target. `cargo xtask libm` reads `src/` only and cannot
see a build script, so this is the rule's reach rather than its check;
RC-19.5's fingerprint is the check. The build script panics on nothing a
document can reach: it reads no input.

#### The surface

`tinker_pdf_color::icc::SRGB_PROFILE` is the bytes, and
`tinker_pdf::SRGB_PROFILE` the same constant on the facade (ruling 11; the
bindings project it in the row that lands each). A caller states it where a
profile is stated today:

- as the archival destination,
  `ArchivalProfile { destination_profile: SRGB_PROFILE.to_vec(), destination_space: DeviceSpace::Rgb, .. }`,
  which `pdfa/colour.rs` admits under every part: version below 3.0, class
  `mntr`, space `RGB `;
- as an `/ICCBased` stream of `/N 3` named by a scope's `/DefaultRGB`, so
  that `DeviceRGB` under a CMYK output intent paints in a characterised space
  (ISO 19005-2 6.2.4.3, part 1's 6.2.3.3).

### Milestones

1. **RC-19.1: this section.**
2. **RC-19.2: the profile.** `build.rs`, `icc::SRGB_PROFILE`, and the
   `icc_profile` fuzz target's seed `srgb-first-party.icc`, written by
   `write_the_fuzz_seeds` as one that reaches `Transform`. Exit:
   `crates/tinker-pdf-color/tests/srgb_profile.rs`'s
   `the_colorants_are_iec_61966_2_1s_primaries_adapted_to_d50` (the columns
   recomputed in the test from the table above, each equal to its
   `s15Fixed16Number` within the step's rounding),
   `the_trc_is_iec_61966_2_1s_transfer_function` (every entry recomputed),
   `the_profile_parses_compiles_and_is_srgb` (`Model::MatrixTrc`, every grey
   level and the three primaries back within one level) and
   `the_header_and_tags_are_what_rc_19_1_lists`.
3. **RC-19.3: the facade's constant.** Exit: `pdfa_writer.rs`'s
   `the_first_party_srgb_profile_is_a_destination_every_flavour_admits` and
   `pdfa_colour.rs`'s `a_first_party_srgb_default_rgb_excuses_device_rgb`.
4. **RC-19.4: the record moves with it.** THIRDPARTY.md, `ArchivalProfile`'s
   type note, [pdfa](pdfa.md), `features/pdfa.md`, `features/fonts.md`,
   `features/rendering.md` and the CMAP-3 reason in
   `tinker-pdf-shape/tests/text_rendering.rs` say a first-party profile is
   computed and handed out, the ICC's files still refused. Exit:
   `cargo doc --workspace --no-deps` with no broken intra-doc link, and the
   `rg` of RC-19.4's row finding only sentences that name NG-03's files or
   this profile.
5. **RC-19.5: one byte string on every target.** Exit: `determinism.rs`'s
   `the_first_party_srgb_profile_is_one_byte_string_on_every_target`, the
   SHA-256 of `tinker_pdf::SRGB_PROFILE`, entered in K-07's roster.

### Risks

| Risk | Mitigation |
| --- | --- |
| A build host computes other bytes | Four IEEE operations and `tinker_pdf_math::pow`; RC-19.5 pins the SHA-256 on every target, and a host that differs is a determinism bug in `build.rs`, never a table edit |
| The numbers are a draft's | The standard's, from a publication of its final values; the threshold is 0.04045, the one the tree already reads |
| The profile is not the sRGB the engine converts to | `the_profile_parses_compiles_and_is_srgb` takes every grey level and the primaries through the engine's own CMM back to themselves |
| A part 1 validator refuses the version | 2.4.0 is below 3.0, which `header_defects` admits under every part; RC-19.3 holds every flavour to it |
| A reader that takes `chad` to undo the adaptation sees D65 as the display's white | That is sRGB's white, as Annex E describes the tag; this engine does not read it |
| A licence question returns | The bytes are computed from published numbers by code in this repository; NG-03's files stay out |
