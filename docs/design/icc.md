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
- **Rendering intents** (8.6.5.8): `/Intent` on the `ICCBased` stream, the
  `ri` operator and `/RenderingIntent` in ExtGState select nothing; the
  transform takes the default intent ([Rendering intents](#rendering-intents)).
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
  separations are [ROADMAP](../ROADMAP.md) rows RC-03 and RC-04.
- **Black point compensation, gamut mapping beyond intent selection,
  iccMAX/v5 profiles, named-colour tags.** Refused by name, per ruling 2;
  [ROADMAP](../ROADMAP.md) rows RC-02 and RC-20.
- **Profiles embedded inside image codecs.** JPX colour boxes have their own
  path in `tinker-pdf-filters`; routing them through this CMM is scheduled by
  corpus evidence (ruling 3), not assumed here.
- **A bare `/CalGray` or `/CalRGB` name.** `parse_space` reads both arrays'
  white point, gamma and matrix (8.6.5.2–8.6.5.3); a bare name, which carries
  no parameters, is drawn as its device space.
- **For default colour spaces, a JPEG or JPEG 2000 image's colour.** Such an
  image is drawn in the colour its codestream carries and never reads its
  `/ColorSpace`: an image XObject, and an inline one, returns before the space
  is read. Neither a default nor a space named directly reaches it — the gap
  P-22's notes record for an ICC-based CMYK JPEG. Closing it moves every
  profiled JPEG a document draws, which is a row's own question rather than
  K-02's.
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

**Compilation, then integer evaluation.** `Transform::compile(&Profile)` runs
once per profile and produces: per-channel curves sampled to 4 096-entry
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

**Wiring.** `ColorSpace::Icc { transform: Arc<Transform>, components }` is
built by the `ICCBased` arm from the stream bytes and `/N`, and any profile
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

### Default colour spaces (8.6.5.6)

Roadmap row K-02. A resource dictionary's `/ColorSpace` may carry
`/DefaultGray`, `/DefaultRGB` and `/DefaultCMYK` (PDF 1.1), and 8.6.5.6 makes
each stand in for the device space it is named for, so that device colours
are drawn calibrated. Rendering substitutes them on every painting path that
reads a space (`crates/tinker-pdf/tests/default_spaces.rs`), and
`crates/tinker-pdf/tests/default_space_census.rs` counts them by scope; the
PDF/A and PDF/X validators read them on their own walk
(`crates/tinker-pdf/src/pdfa/colour.rs`, `crates/tinker-pdf/src/pdfx/print.rs`).
Reporting the substitute in extraction is roadmap row K-02.3, and the
census's first figures K-02.5. This section transcribes what the standard
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
| An image XObject's `/ColorSpace` | yes; **a JPEG or JPEG 2000 image is drawn without reading it** | 8.6.5.6 |
| An inline image's `/CS` | yes, after `expand_inline_abbreviations`; **an inline JPEG or JPEG 2000 is drawn without reading it** | 8.6.5.6, 8.9.7 |
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
any default. Reporting the space its samples are in beside it is roadmap row
K-02.3, designed here:

- **`PageImage::substituted_space`** is the stated space with each device
  space in it that the scope remaps replaced by the default, described as
  `describe_space` describes any space: an image in
  `[/Indexed /DeviceRGB …]` under a `/DefaultRGB [/ICCBased …]` is
  `Indexed { base: Icc { … } }`. It is `None` where nothing was replaced, so
  a page with no default lists exactly what it lists without one.
  `PageImage::sample_space` answers the space the samples are in — the
  substituted one where there is one, the stated one otherwise — and is what
  K-02.3's twins compare.
- **One entry per image and space.** `Collector` lists an XObject once, by
  its reference, with every placement. Under defaults one XObject can be drawn
  in two spaces — on a page with a `/DefaultRGB` and in a form whose own
  `/Resources` have none — and is then listed once for each, each with its
  own placements. An inline image is its own entry.
- **A JPEG or JPEG 2000 image reports its substitute like any other.**
  Extraction describes the space the standard puts the samples in, which is
  the default's, though the renderer reads neither space (the non-goal
  above).
- **A refused default replaces nothing**, and an image in its family names
  the refusal in `warnings`, as it names a decoder's leniency (ruling 10). A
  soft-mask image and a stencil have nothing substituted.
- **`to_svg` follows the render with no change of its own.** It writes the
  interpreter's `fill_color` (`crates/tinker-pdf/src/svg_out.rs`) and decodes
  images and shadings through the `GlyphSource` calls the renderer makes, so
  the substitution reaches it; K-02.3's SVG test holds that.

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

A JPEG or JPEG 2000 image reads no `/ColorSpace` (the non-goal above). A
default cannot reach it, and nor can the space its twin names directly, so a
twin comparison over a JPEG passes with neither applied; the twins in
`default_spaces.rs` use sampled images for that reason.

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

### Rendering intents

`/Intent` on the `ICCBased` stream, the `ri` operator and ExtGState
`/RenderingIntent` select nothing, and `ri` is parsed and discarded. An intent
only changes an answer at all for a LUT profile carrying more than one `A2B*`
table, and the transform takes 8.6.5.8's default, `RelativeColorimetric` —
which is `A2B1`, which is what the parser reaches for first. Every intent is
[ROADMAP](../ROADMAP.md) row RC-01.

The wrinkle: an intent is graphics state and a transform is compiled per
colour space, so a space would need one compiled transform per intent rather
than one. That is the reason it is not a two-line change.

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
  `/DefaultCMYK`. Milestone 12's corpus run depends on MS-11 and H-03, as
  K-02.5 lists them.

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
| A twin comparison over a JPEG passes with nothing applied and is taken for proof | The twins use sampled images, and the JPEG path is a named non-goal |
