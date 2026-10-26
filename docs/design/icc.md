# ICC colour management

When this is done, the two colour claims the engine currently makes on its
own types will be retired: `ColorSpace::Approximated` in
`crates/tinker-pdf-color/src/lib.rs` says ICC and CIE spaces are honoured
only by component count, and `tinker_pdf_content::Group`'s doc comment says
the group's colour space is "deliberately absent" because the renderer
composites in RGB throughout. The replacement is an own CMM — profile
parsing and transform evaluation in the `tinker-pdf-color` leaf, no
third-party colour engine — and transparency-group colour spaces honoured
in compositing (ISO 32000-1 11.4.7, 11.6.6), including the page-level
`/Group` that `Page::render` does not read today. The
roadmap named both gaps and pointed them here, and the entry has since left
it; the exit it recorded is that ICC profiles drive conversion and that
known-answer tables computed from the specification's own equations hold
([render-verification](render-verification.md)). What is still open of this
design — the refused profiles — is listed in the [roadmap](../ROADMAP.md) on
its own; the two ledger rows closed on 4 September 2026 and the note below
records what they measured.

## Scope

- **Group colour spaces first.** `/Group /CS` on form XObjects (11.6.6)
  and on the page object (11.4.7) read, resolved through the existing
  `parse_space` seam in `crates/tinker-pdf/src/resources.rs`, and honoured:
  blending happens in the declared space, conversion happens once at group
  boundaries, never per element.
- **An own CMM second**, as an `icc` module of `tinker-pdf-color`: profile
  header and tag table parsing (ICC.1:2010, ISO 15076-1; v2 profiles
  included, since PDFs embed them constantly), TRC curves (`curv`,
  `para`), matrix/TRC transforms (`rXYZ`/`gXYZ`/`bXYZ`, `wtpt`, `chad`),
  LUT transforms (lut8 `mft1`, lut16 `mft2`, `mAB `, `mBA `), compiled
  into a profile→PCS→destination pipeline.
- **Rendering intents** (8.6.5.8): `/Intent` on the `ICCBased` stream, the
  `ri` operator — parsed and discarded today at
  `crates/tinker-pdf-content/src/interpret.rs:696` — and `/RenderingIntent`
  in ExtGState, selecting which A2B/B2A table the transform compiles.
- **Determinism under ruling 4** ([rulings.md](../rulings.md)): transform
  *evaluation* is integer and fixed-point only; anything transcendental
  happens once at table-construction time via `tinker-pdf-math`.
- **Typed refusals under rulings 2 and 10**: a profile the parser declines
  falls back to today's `Approximated` with a warning naming the object
  and the reason, never a hard failure.
- Fixtures, fingerprints, a fuzz target and specification-derived
  known-answer tables for all of the above
  ([verification.md](../verification.md), ruling 13).
- **Default colour spaces** (8.6.5.6; roadmap row K-02): a scope's
  `/DefaultGray`, `/DefaultRGB` and `/DefaultCMYK` stand in for the device
  space each is named for, on every path that selects one, when rendering
  and when extracting. Designed in
  [Default colour spaces](#default-colour-spaces-8656) below. It waits on
  no milestone here and builds on both stages: a default is a CIE or
  `ICCBased` space like any other, and stage 2's CMM converts it.

## Non-goals

- **Output intents and proofing** (14.11.5): `/OutputIntents` handling
  belongs to [pdfa](pdfa.md); this design converts for the screen, to
  sRGB, always.
  *No longer a non-goal, 9 October 2026 (ruling 3 as amended)*: soft proofing is [ROADMAP](../ROADMAP.md)
  row RC-05.
- **N-channel output, overprint and spot-ink simulation.** `Separation` /
  `DeviceN` keep flowing through their tint transforms (8.6.6.4, 8.6.6.5);
  simulating ink interaction is a different renderer.
  *No longer a non-goal, 9 October 2026 (ruling 3 as amended)*: overprint simulation and separations are
  [ROADMAP](../ROADMAP.md) rows RC-03 and RC-04.
- **Black point compensation, gamut mapping beyond intent selection,
  iccMAX/v5 profiles, named-colour tags.** Refused by name, per ruling 2.
  *No longer a non-goal, 9 October 2026 (ruling 3 as amended)*: [ROADMAP](../ROADMAP.md) rows RC-02 and
  RC-20.
- **Profiles embedded inside image codecs.** JPX colour boxes already have
  their own path in `tinker-pdf-filters`; routing them through this CMM is
  scheduled by corpus evidence (ruling 3), not assumed here.
- **Exact CalGray/CalRGB.** `parse_space` maps them to device spaces
  today; 8.6.5.2–8.6.5.3 fidelity rides on the same PCS machinery and can
  follow, but no milestone below depends on it.
  *Built, 7 September 2026*: `parse_space` reads both arrays' white point,
  gamma and matrix (`crates/tinker-pdf/src/resources.rs:1075-1111`); only a
  bare `/CalGray` or `/CalRGB` name, which carries no parameters, is still
  drawn as its device space.
- **For default colour spaces, a JPEG or JPEG 2000 image's colour.** Such an
  image is drawn in the colour its codestream carries and never reads its
  `/ColorSpace`: an image XObject returns at
  `crates/tinker-pdf/src/resources.rs:3135-3166`, before the space is read at
  `:3176`, and an inline one at `:3500-3524`, before `:3530`. Neither a
  default nor a space named directly reaches it — the gap P-22's notes record
  for an ICC-based CMYK JPEG (`docs/roadmap/P.md:840`). Closing it moves
  every profiled JPEG a document draws, which is a row's own question rather
  than K-02's.
- **For default colour spaces, the scope a Type 3 glyph or a soft-mask group
  runs in.** Both run in the scope that invokes them rather than in their own
  `/Resources` (see [the scope a default is read in](#the-scope-a-default-is-read-in)).
  A default follows the scope the engine resolves their other names in; the
  scope is the gap, and it is not this design's.
- **For default colour spaces, PDF/A's and PDF/X's reading.** The validators
  record a default per walk rather than per resource dictionary
  (`crates/tinker-pdf/src/pdfa/colour.rs:800-819`), a simplification they
  name; this design changes rendering and extraction, not validation.

## Design

### Stage order: group spaces before the CMM

The group gap is the one that blocks *correct compositing structure*: a
`Multiply` between two colours inside a CMYK group is a different formula
from the same blend in RGB, whatever profile fidelity each colour had. The
CMM changes the exact shade of managed colours; the group space changes
which numbers the blend formulas ever see. Groups are also the smaller
change, and stage 2 slots into the seam stage 1 builds (an ICCBased group
`/CS` becomes just another space the boundary conversion handles). So:
groups first, CMM second.

### What the corpus carries, measured before any of this was built

Ruling 3 wants a capability scheduled by what real documents need. The census in
`crates/tinker-pdf/tests/icc_census.rs` reads the header and tag table of every
ICC profile in the four corpora — bytes only, sharing no code with the colour
crate, so it cannot agree with a parser that is wrong.

**2 313 of 4 605 files carry a profile**, 2 750 profiles in all. That is half
the corpus, and a far higher reachability than anything else in this tier: JPX
had 19 files and JBIG2 103.

| | profiles |
| --- | ---: |
| matrix/TRC — three `XYZ` columns and three tone curves | **2 287** |
| grey — a single `kTRC` and a white point, no matrix | 323 |
| needing a LUT (`A2B*` / `B2A*`) | **140** |

| | files |
| --- | ---: |
| every profile matrix/TRC | 1 932 |
| carrying any LUT profile | 131 |

**Two different numbers, and the difference matters.** 2 313 files carry a
profile *stream*; only **449 name an `ICCBased` colour space** that paints
anything. The gap is `/OutputIntent`: a PDF/A file declares the space it was
prepared for whether or not any content is painted through it, which is why
366 of the 449 and the great bulk of the 2 313 are veraPDF's. So "half the
corpus carries a profile" is true and is not the reachability figure — the one
that measures what this capability changes on a page is 449 files, 9.9 %,
which is still the highest of any capability the corpus report tracks.

So **matrix/TRC and grey together are 95 % of the profiles**, and the LUT
machinery this document sizes at L on its own is the remaining 5 %. That splits
the stage cleanly and puts the LUT milestone after the wiring rather than before
it: a build that transforms matrix and grey profiles and refuses LUT ones by
name is right about nineteen profiles in twenty.

Two more numbers the milestones need. The device classes are `mntr` 2 428,
`prtr` 316, `scnr` 5; the data spaces are RGB 2 286, GRAY 323, CMYK 138, Lab 2.
And the versions are **v2 2 739, v4 9, v5 2** — so v2 is not a legacy case to
tolerate, it is the case, and v4's structural additions are worth exactly nine
files. The largest profile in the corpus is **718 672 bytes** and the busiest declares
**eighteen** tags, which are the two figures the ICC rows of
`bounds_ledger.rs` have to clear.

### Stage 1: transparency-group colour spaces

**Plumbing.** `tinker_pdf_content::Group` (interpret.rs:50) gains
`space: Option<tinker_pdf_color::ColorSpace>` — the crate already depends
on `tinker-pdf-color`, and the resource seam already resolves `/Group /CS`
once, for the `/BC` backdrop at resources.rs:972. The form-XObject arm at
resources.rs:1283 fills it; a new read in `Page::render`
(`crates/tinker-pdf/src/lib.rs:871`) resolves the page dictionary's
`/Group` (7.7.3.3) and, when `/S /Transparency`, opens a page-level group
frame in the render device before the content stream runs — reusing
`open_group`/`close_group` in `crates/tinker-pdf-render/src/lib.rs`
untouched, including `MAX_GROUP_DEPTH` and the `MAX_GROUP_BUFFERS` budget.

**Blending in the declared space.** The group buffer holds components of
the group's space plus alpha. `PixelFormat` in
`crates/tinker-pdf-raster/src/canvas.rs` (`Gray8`/`GrayA8`/`Rgb8`/`Rgba8`)
grows `CmykA8`; supported group spaces are keyed by shape — one channel,
three, four — which covers `/DeviceGray`, `/DeviceRGB`, `/DeviceCMYK` and
(stage 2) `ICCBased` by `N`. A group declaring `/Lab` is composited in RGB
with a typed warning: blending in a space whose components are not in 0..1
is a decision this design refuses rather than half-takes, and the corpus
will say whether it matters (ruling 3). Separable blend modes (11.3.5)
already run per channel in `crates/tinker-pdf-raster/src/blend.rs`; the
formulas assume additive components, so CMYK channels enter and leave them
complemented. Non-separable modes (11.3.5.3) are defined on RGB values:
on a CMYK buffer the operands convert to RGB, blend, and convert back,
recorded as an approximation with a typed warning.

**Conversion at boundaries only.** `tinker-pdf-color` gains
`convert(from: &ColorSpace, components, to: &ColorSpace)`, used at exactly
three points: painting a source colour into a group whose space differs
(source components → group space), initialising a non-isolated group's
backdrop (parent space → group space, 11.4.4), and `close_group`
compositing the finished buffer onto the parent (group space → parent
space). In stage 1 the device relations of 8.6.4.4 supply the forward
direction and their inverse — RGB→CMYK with `k = min(1-r, 1-g, 1-b)`
undercolour removal — supplies the backward one; stage 2 replaces both ends
with profile transforms without moving the call sites.

**That inverse is exact, not an approximation, and this paragraph used to say
otherwise.** Maximum undercolour removal makes each channel's intermediate
error smaller than half a level, so RGB→CMYK→RGB is the identity for all
sixteen million colours — swept exhaustively as 32 896 `(v, max)` pairs, since
`K` is fixed by the maximum and each channel is then independent. What *is*
approximate is the other direction: a CMYK value that did not come from the
inverse — a rich black — comes back as its pure-K equivalent. **Since October
2026 such values exist**: a DeviceCMYK colour's own components cross the
resource seam beside its light (`FontSource::resolve_ink`,
`GraphicsState::fill_ink`), and a CMYK buffer — a `/DeviceCMYK` group's, or a
page asked for in ink — composites them. Leaving a CMYK group for an additive
parent takes them through 8.6.4.4 forward, which needs no inverse and is exact;
a non-isolated group over a parent of its own format adopts the backdrop as the
parent's bytes and takes the same bytes out at close (`Canvas::adopt_backdrop`,
`remove_backdrop`), where until the lane's review both went through light and
a rich black under the group was blended against as pure K; what does not
round-trip is a non-separable blend inside the group, whose operands go to
light and back, and `RenderWarning::ApproximatedGroupBlend` already names
that. Soft-mask luminosity (11.6.5.2) reads the group's
own space — but **not for free, and not where this doc expected**. `to_mask`
reads through `Canvas::pixel`, which already applies the group's own relation,
so the space arrives on its own. The real defect was the *weighting*:
11.6.5.2's luminosity is 11.3.5.3's `Lum`, and the code reached for
`Color::luma`'s Rec.601 coefficients instead of the clause's 0.3/0.59/0.11 —
the very pair `blend.rs` records as "the difference between matching a
reference renderer and not". They differ by a level on a saturated colour and
not at all on a grey, which is why no fixture had noticed. The `/BC` default
needed nothing: `Rgb::BLACK` is right in every space, Lab included, because
`L = 0` converts to RGB(0,0,0) like every other black.

**Proof.** Pairs that differ only in the group's `/CS`, asserted *unequal*.

**Not `Multiply`, which is what this paragraph first proposed.** Writing the
ink split as `k = 1 - max(r,g,b)` makes the complemented components exactly
`(R/max, G/max, B/max, max)`, so a separable blend `f` recombines to
`R' = f(R1/max1, R2/max2) * f(max1, max2)`. For a product the two `max` terms
cancel and the answer is `R1 * R2` — RGB's answer. A multiplied pair renders
*identically* in both spaces and would have passed on a build that ignored
`/CS` altogether. Measured differences, in levels of 255: Normal and Multiply
0 when opaque, Darken 8.5, Lighten 29, Screen 36, Difference and Exclusion 255;
every mode differs under partial alpha.

So `Difference` carries the pair, and the invariance is kept as a *second*
test rather than discarded: an opaque `Multiply` must agree in both spaces
within a level. That is the assertion that catches a complement applied on one
side of the blend and not the other — and it earned its place, because
removing the complement leaves the `Difference` pair passing and fails only the
invariance one.

### Stage 2: the CMM

**The leaf boundary, argued.** Ruling 8 wants bytes and plain parameters
in, values out. Profile parsing fits exactly: `icc::Profile::parse(&[u8])`
sees ICC bytes, never a COS stream — the facade's `ICCBased` arm
(resources.rs:449) decodes the stream and hands bytes across, as it hands
function dictionaries across today. Evaluation lives in the same leaf, and
the precedent is `Function::eval` in
`crates/tinker-pdf-color/src/function.rs`: the leaf owns both the parse
and the interpreter because the tables are meaningless without the exact
interpolation defined over them, because the fuzz target must drive
parse *and* eval to prove never-panic on hostile profiles, and because the
alternative — handing raw tables to `tinker-pdf-render` to interpret —
moves ICC vocabulary into the renderer and splits ruling 4's obligations
across two crates. The renderer keeps calling `ColorSpace::to_rgb` and
`convert`, unchanged.

**Compilation, then integer evaluation.** `Transform::compile(&Profile,
Intent, Direction)` runs once per profile and produces: per-channel input
and output curves sampled to 4 096-entry `u16` tables, a 3×3 matrix in
s15.16 fixed point (ICC's own `s15Fixed16Number`, so matrix-based
profiles are *natively* fixed point), and for LUT profiles a CLUT walked
with integer trilinear interpolation. Sampling a `para` curve needs
`pow` — that is `tinker_pdf_math::pow`, exactly as `lab_to_rgb` uses it
today, because `tinker-pdf-color` is on the `PIXEL_PATHS` list that
`cargo xtask libm` enforces (xtask/src/main.rs:431) and a platform `powf`
in a TRC is precisely the cross-target divergence ruling 4 exists to stop.
Per-pixel evaluation touches no float at all. The PCS leg reuses the
existing D50 XYZ→sRGB path; `mAB`/`mBA` profiles with a Lab PCS go through
the `finv` machinery `lab_to_rgb` already owns, sampled into the same
integer tables at compile time.

**Wiring.** `ColorSpace` gains
`Icc { transform: Arc<Transform>, components }`; the `ICCBased` arm builds
it from the stream bytes and `/N`, keeping today's `Approximated`
fallback — now accompanied by a ruling-10 warning naming the object and
the refusal (bad signature, unsupported tag, dimension over bounds) — for
any profile the parser declines. The `ri` operator and `/Intent` select
the compiled table set. Two new rows land in
`crates/tinker-pdf/tests/bounds_ledger.rs`, measured against real embedded
profiles. They landed as maximum accepted profile size and maximum **tag
count** rather than maximum CLUT grid volume: `MAX_CLUT_ENTRIES` refuses by
name and is reachable, but the yardstick that would give it a row is a grid
volume, and the corpus census counts tags and bytes.

### Verification

- **Fuzzing:** an `icc_profile` cargo-fuzz target lands in the same PR as
  the parser, per the standing rule in
  [verification.md](../verification.md), seeded with the profiles the
  fixtures embed.
- **Known-answer tables, not a second CMM.** Ruling 13 forbids asking
  another colour engine what a transform should produce, so the answers
  come from the specification instead. For a matrix/TRC profile every step
  is published arithmetic — the parametric curve types of ICC.1 6.2, the
  s15.16 matrix, the chromatic adaptation of Annex E — so a grid of inputs
  has expected outputs computable by hand and committed as a table, in
  `crates/tinker-pdf-color/tests/icc_known_answers.rs`. Each table records
  the clause its numbers come from. This is *stronger* than a budgeted
  agreement with another CMM, which is why the milestone that used to buy
  agreement now buys exactness.

  **What it does not reach:** LUT profiles (`mft1`, `mft2`, `mAB `,
  `mBA `). Their CLUT interpolation is where two CMMs legitimately differ,
  and it is exactly the part no published table settles. Hand-computed
  lookups pin the interpolation this engine chose; nothing says that choice
  matches what a profile's author expected. That is a named limit of this
  design, recorded in [verification.md](../verification.md)'s terms.
- **Corpus evidence:** the `tpdf probe` capability scan
  (`scan_capabilities` in tools/tpdf/src/main.rs) grows an `iccbased` tag beside `jbig2` and
  `jpx`, so `corpus/report.json` measures how many real files carry
  profiles, and *which kinds* — the ruling-3 evidence that schedules the
  LUT milestone.
- **Fingerprints:** the stage-1 pairs plus new ICCBased fixtures are
  committed fingerprints in `determinism.rs`, holding ruling 4's
  bit-identical contract across targets once profiles drive conversion.

### Default colour spaces (8.6.5.6)

Roadmap row K-02, designed on 25 October 2026; rendering built the same day
(K-02.2, milestone 9), extraction and the census not yet. A resource
dictionary's `/ColorSpace` may carry `/DefaultGray`, `/DefaultRGB` and
`/DefaultCMYK` (PDF 1.1), and 8.6.5.6 makes each stand in for the device
space it is named for, so that device colours are drawn calibrated. Nothing
outside the PDF/A and PDF/X validators reads them today
(`crates/tinker-pdf/src/pdfa/colour.rs:800-819`,
`crates/tinker-pdf/src/pdfx/print.rs:187-190`): a page that calibrates its
device colours through one is drawn as though it had not, and the space the
default names is never asked. This section transcribes what the standard
says, maps every path in the tree that selects a device space and every
scope a default is read in, and settles what K-02.2 to K-02.5 build.

**Quoted from ISO 32000-1:2008 with no copy of it in the tree**, so the
wording is a reviewer's to check against the standard. Where the standard is
silent, or this section is not sure what it says, it says so, and each such
place carries the decision this design takes and the test that holds it, so
a different reading moves one test.

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

| Selection | Remapped | Said by | This engine today |
| --- | --- | --- | --- |
| `g`/`G`, `rg`/`RG`, `k`/`K` | yes | 8.6.8 | converted in place, never asking the resource seam (`crates/tinker-pdf-content/src/interpret.rs:1022-1050`) |
| `cs`/`CS` naming a device space, or a `/ColorSpace` entry that is one | yes | 8.6.5.6 | `color_space` (`crates/tinker-pdf/src/resources.rs:922-958`); `parse_space`'s name arm (`:967-975`) |
| `sc`/`scn` in such a space, or with no space named | yes | 8.6.5.6 | `interpret.rs:1071-1131`; with none named, read by count (`:1120-1126`, `components_to_rgb` at `:1874`) |
| An `/Indexed` base | yes | 8.6.5.6 | `parse_space` (`resources.rs:1010-1029`) |
| `[/Pattern base]`'s base | yes, in the scope whose `cs` selected it | 8.6.5.6 | `resources.rs:1064-1068`; `scn`'s operands (`interpret.rs:1087-1108`) |
| A `/Separation` or `/DeviceN` alternate | when selected — which, as this section reads 8.6.6.4, `/All` and `/None` never are | 8.6.5.6 | always selected, since no render paints a named colorant on a plate of its own, the ink render's process plates included, and `/All` and `/None` are not told apart (`resources.rs:1030-1057`; RC-04.5) |
| An image XObject's `/ColorSpace` | yes | 8.6.5.6 | a sampled image's at `resources.rs:3176-3179`; **a JPEG or JPEG 2000 image returns before it is read** (`:3135-3166`) |
| An inline image's `/CS` | yes | 8.6.5.6, 8.9.7 | `resources.rs:3530-3533`, after `expand_inline_abbreviations` (`:319-376`); **an inline JPEG or JPEG 2000 returns before it** (`:3500-3524`) |
| A shading's `/ColorSpace`, under `sh` and in a shading pattern, and its `/Background` with it | yes | 8.6.5.6 | `read_shading` (`resources.rs:1181-1184`) |
| A transparency group's `/CS`, a form's and the page's | yes | 11.6.6 | `resources.rs:2102-2106` and `:653-674`, reduced to a shape by `group_space` (`:385-398`) |
| A luminosity soft mask's `/BC` | yes, through `/G`'s `/CS` | 11.6.5.2, 11.6.6 | `resources.rs:1626-1645` |
| A stencil mask, and an image's `/Mask` | nothing to remap | 8.9.6.2, 8.9.6.3 | — |
| A soft-mask image (`/SMask`) | no | read here from 11.6.5.3 | — |
| A JPEG 2000 codestream's own colour, under no `/ColorSpace` | no | read here | — |
| A space inside a default | not stated; no, here | — | — |
| A `/DeviceN`'s `/Process` space | not stated | 8.6.6.5 | not read (RC-04.5) |

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
other respect (`resources.rs:925-955`), so a default applies to them as it
does to the names they stand for.

#### The scope a default is read in

**A default is read from the dictionary the stream's other names resolve
in.** 8.6.5.6 says "the current resource dictionary", and the engine keeps
one object per such dictionary, `PageResources`. Reading the default from
the object a `cs /CS0` beside it resolves against means the two are never
read in different scopes, including in the two rows below where the
engine's scope is itself the wrong one.

| Scope | The standard | This engine |
| --- | --- | --- |
| Page | its `/Resources`, inherited down the page tree (7.7.3.4) | `PageResources::new` (`resources.rs:574`), over the inherited dictionary (`crates/tinker-pdf-cos/src/pages.rs:235-239`) |
| Form XObject | its own `/Resources` (8.10.1); without them, the page's, a construct 7.8.3 calls obsolete | its own (`form_resources`, `resources.rs:857-891`), or the invoking scope when it has none (`interpret.rs:1470-1475`) |
| Tiling pattern | its own `/Resources`, required (Table 75) | its own, or the invoking scope's when absent (`resources.rs:1860-1873`) |
| Type 3 glyph | the font's `/Resources`, or the page's when absent (9.6.5) | **the enclosing scope**: a glyph procedure runs without changing scope (`interpret.rs:1777-1800`), which `docs/features/editing.md:844` records for subsetting |
| Annotation appearance | its own `/Resources`, an appearance stream being a form XObject (12.5.5) | its own, or an empty scope when it has none (`crates/tinker-pdf/src/annots.rs:234-238`) |
| Soft-mask group | `/G`'s own `/Resources`, `/G` being a form XObject (11.6.5.2) | **the scope that set the `gs`** (`interpret.rs:1597-1657`), the deviation FT-28's notes record (`docs/roadmap/FT.md:1150`) |

- **A page's default does not reach into a form that has `/Resources` of
  its own**, even resources with no `/ColorSpace`: they are the current
  resource dictionary inside it, and 8.10.1 makes an independent form's
  resources the whole of what it may name. A reader that carries a scope's
  defaults into the forms it invokes draws such a form in the substitute
  where this design draws it in the device space; K-02.4 counts the files
  where the two part. The other way, a form's default applies inside it
  only.
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
the invoking stream's, where `Do` runs. This design reads the scope the
group's contents run in: the form's own when it has them, the invoking scope
when it has not, and for a soft-mask group the scope the engine runs it in.
A group whose `/CS` were remapped by one dictionary and its contents by
another would paint them in one calibration and composite them in another.
Today `form_from` reads a form group's `/CS` on the invoking scope
(`resources.rs:2102-2106`); K-02.2 reads it on the form's.

**Remapping a group's `/CS` changes no pixel this engine draws today, and the
`/BC` is where a default shows.** `group_space` keeps only a space's shape —
one, three or four channels, or Lab (`resources.rs:385-398`) — and a default
has its device space's component count, so a remapped group composites in
the same buffer through the same boundary conversions; the remap starts to
matter when a group composites through a profile. A luminosity mask's `/BC`
is converted to light through the space (`resources.rs:1626-1645`), so a
default there moves the backdrop the mask is measured against.

#### One selection, two answers

**The space as selected and the space as substituted.** `resolve_color` and
every decode — images, inline images, shadings, the `/BC` — convert through
the substitute. `resolve_ink` (`resources.rs:1516-1532`) and `initial_color`
(`:1534-1536`) read the space as selected: the first so that a DeviceCMYK
colour keeps its ink under a default (below), the second for the colour `cs`
resets to. Both answers come from one parse of the selection — the substitute
is the parsed space with the scope's defaults put in — so nothing is parsed
twice, and the per-name cache P-02.5 adds keeps the two together.

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
start from different colours, so K-02.2's twins set their components before
they paint.

**A default the clause does not allow is refused, not obeyed**: Lab,
Indexed or Pattern, a component count other than the device space's, or a
value that is no colour space. The device space is drawn as today and the
refusal is named — `RenderWarning::RepairedColorSpace` under the default's
own name, once per render of the scope (ruling 10). 8.6.5.6 says what may be
a default and not what a reader does with one that may not; drawing what the
content asked for, and saying so, is ruling 2's answer.

**The device operators leave the space slot as they found it, and must
not.** `g`, `rg` and `k` pass no space to `set_color`, which then leaves
`fill_space` naming whatever `cs` set last (`interpret.rs:1387-1404`), so
`/CS1 cs 1 sc 0.5 g 0.7 sc` paints `0.7` in `/CS1` where 8.6.8 has `g`
select DeviceGray. Measured on this tree with a scratch probe, over
`/CS1 [/Indexed /DeviceRGB 1 <FF0000 0000FF>]`: the fill is `(0, 0, 255)`,
where the same page without `/CS1 cs 1 sc` paints DeviceGray's
`(179, 179, 179)`.
The slot is what `sc` asks the default by, so K-02.2 sets it to the reading
by count (`interpret.rs:1120-1126`), which allocates nothing, and the reading
by count asks the default of the family it takes the components as: one is
gray, four is CMYK, any other count is RGB.

#### `k` under `/DefaultCMYK` on an ink render (H-135)

[H-135](../roadmap/H.md) is the owner's open question: on an ink render
(`format: PixelFormat::CmykA8` with `RenderOptions::allow_cmyk`), does a
DeviceCMYK colour under a `/DefaultCMYK` keep the document's own components
as ink, or is it converted through the default like every other colour and
lose them? This section does not settle it. K-02.2 builds H-135's default
until the owner decides:

- **The light render paints the default.** `k` under
  `/DefaultCMYK [/ICCBased …]` paints the profile's light, as `/CS0 cs … sc`
  naming that profile directly does.
- **An ink render keeps the document's ink**, as it does today: `1 1 1 1 k`
  arrives as all four inks, from `fill_ink`
  (`crates/tinker-pdf-content/src/state.rs:398-410`). The light beside it,
  which an ink page reads wherever it needs light rather than ink — inside a
  group whose `/CS` is `/DeviceRGB`, say — is the default's.
- **The same for every flat DeviceCMYK colour**, which is everything that
  carries ink today ([rendering.md](../features/rendering.md), "CMYK page
  output"): a fill, stroke or glyph colour set by `k` or `K`, by `sc`/`scn`
  in a space selected as DeviceCMYK, or by four components with no space
  named, and a stencil mask painted in one.
- **A light render composites in ink inside a `/DeviceCMYK` group**, found
  building K-02.2. The renderer composites a colour's ink wherever a canvas
  composites in ink: an ink page, and a `/DeviceCMYK` group's buffer on any
  render (`crates/tinker-pdf-render/src/lib.rs:2156-2161`). So inside such a
  group `k` under a `/DefaultCMYK` paints, on a light render, what the page
  without the default paints, not what its twin naming the profile paints,
  and the first bullet does not hold there. It follows from keeping the ink,
  and choosing conversion would change it with the rest; a guard row of
  `an_ink_render_keeps_a_device_cmyk_colour_s_ink_under_default_cmyk` holds
  it until the owner decides.
- **What carries no ink today is not H-135's question.** An image, a
  shading, an uncoloured pattern's colour, and an `/Indexed` or separation
  space over DeviceCMYK reach an ink canvas as light taken back to ink;
  under a default that light is the default's, and there was no ink of the
  document's to keep.
- **The two contracts H-135 names part on such a page.** `Bitmap::to_png`
  writes an ink page as 8.6.4.4's light, and the light render paints the
  profile's, so a page painting `k` under `/DefaultCMYK` writes a PNG that is
  not its light render.
  `an_ink_page_written_as_png_is_the_light_the_switch_would_have_returned`
  (`crates/tinker-pdf/tests/render_options.rs:536`) holds two pages that
  carry no default and says nothing of this one. Choosing conversion
  instead is one later change to `resolve_ink`, as H-135 says.

#### What `Page::images` reports for an image drawn under a default

- **`color_space` stays the space the image states**
  (`crates/tinker-pdf/src/images.rs:87-90`): a `/DeviceRGB` image is
  `DeviceRgb` under any default.
- **`PageImage::substituted_space` beside it** is that space with each
  device space in it that the scope remaps replaced by the default, described
  as `describe_space` describes any space (`resources.rs:3882`): an image in
  `[/Indexed /DeviceRGB …]` under a `/DefaultRGB [/ICCBased …]` is
  `Indexed { base: Icc { … } }`. It is `None` where nothing was replaced, so
  a page with no default lists exactly what it lists today.
  `PageImage::sample_space` answers the space the samples are in — the
  substituted one where there is one, the stated one otherwise — and is what
  K-02.3's twins compare.
- **One entry per image and space.** `Collector` lists an XObject once, by
  its reference, with every placement (`images.rs:259-299`). Under defaults
  one XObject can be drawn in two spaces — on a page with a `/DefaultRGB` and
  in a form whose own `/Resources` have none — and is then listed once for
  each, each with its own placements. An inline image is its own entry, as
  now.
- **A JPEG or JPEG 2000 image reports its substitute like any other.**
  Extraction describes the space the standard puts the samples in, which is
  the default's, though the renderer reads neither space (the non-goal
  above).
- **A refused default replaces nothing**, and an image in its family names
  the refusal in `warnings`, as it names a decoder's leniency (ruling 10). A
  soft-mask image and a stencil have nothing substituted.
- **`to_svg` follows the render with no change of its own.** It writes the
  interpreter's `fill_color` (`crates/tinker-pdf/src/svg_out.rs:737`) and
  decodes images and shadings through the `GlyphSource` calls the renderer
  makes (`svg_out.rs:1298-1345`), so K-02.2's substitution reaches it, and
  K-02.3's SVG test holds that.

#### Read once a scope, so a page with none pays nothing per operator

- **Where.** A scope's `Caches` (`resources.rs:63-101`) gains its three
  defaults, read on the first question the scope is asked about a device
  space and shared by every render of it, since `for_one_render` shares
  `caches` (`:546-561`). The read is one `/ColorSpace` lookup and three keyed
  gets; an entry that is present is parsed once, as written, and checked
  against the rules above. A tiling cell's scope is built anew per tile
  (`:1866-1868`), so its read is once per tile, beside drawing the cell.
- **Per selection on the resource side**, a device arm of `color_space` or
  `parse_space` reads the cached answer: on a page with none, a load and a
  branch.
- **Per operator in the interpreter, nothing.** `g`, `rg` and `k` do not ask
  the seam today and must not start to. `FontSource` gains one method,
  `device_defaults`, asked once per scope — when `interpret` starts
  (`interpret.rs:563`) and when a form changes scope (`:1470-1475`), restored
  when it ends (`:1552-1553`) — answering which of the three device spaces
  the scope remaps. Its default answers none, so `recode.rs` and the tests'
  sources behave as they do, and `subset.rs`'s `Recorded` forwards it as it
  forwards the rest of the colour seam (`crates/tinker-pdf/src/subset.rs:963-1003`).
  With none remapped, `g`, `rg` and `k` run today's code behind one branch
  on a field the interpreter holds; with one, that operator resolves through
  `resolve_color` and `resolve_ink` under the device space's own name, the
  way `/DeviceGray cs … sc` already resolves.
- **Measured, not assumed.** K-02.2 times the bench's render rows in
  release and in debug. A page with no default must cost nothing beyond
  noise, and any cost goes to the owner before it lands.

#### Found while writing this section

Three selections a default rides on are wrong before any default is
involved. Each was read from the code and then measured on this tree with a
scratch probe — a 10 × 10 page rendered with default options, and
`Page::images` — and K-02.2 fixes all three, failing first, because each is
a path its twins draw through:

- **`sc` after a device operator** reads in the last `cs`'s space (above).
- **An inline image's `[/I …]`.** 8.9.7 abbreviates the key `/Interpolate`
  and the family `/Indexed` both as `/I`, and `expand_inline_abbreviations`
  expands every `/I` as the key (`resources.rs:330`). So
  `/CS [/I /RGB 1 <FF0000 0000FF>]` becomes `[/Interpolate …]`: its samples
  `0` and `1` paint `(0, 0, 0)` and `(1, 1, 1)` where `[/Indexed /RGB …]`
  paints red and blue, and `Page::images` lists it as
  `Unreadable { family: "Interpolate" }`.
- **An inline image naming a `/ColorSpace` resource**, which 8.9.7 allows.
  `describe_space` resolves the name (`resources.rs:3900-3913`), but
  `decode_inline` hands it to `parse_space`, whose name arm knows only the
  device spaces (`:967-975`), and falls back to DeviceGray (`:3533`):
  `/CS /CS0` over `[/Indexed /DeviceRGB 1 <0000FF 00FF00>]` paints
  `(0, 0, 0)` and `(1, 1, 1)` while `Page::images` lists the palette.

And one it does not fix: a JPEG or JPEG 2000 image reads no `/ColorSpace`
(the non-goal above). A default cannot reach it, and nor can the space its
twin names directly, so a twin comparison over a JPEG passes with neither
applied; K-02.2's twins use sampled images for that reason.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size (S/M/L/XL) |
| --- | --- | --- | --- |
| 1 | `Group.space` resolved: form `/Group /CS` and page-level `/Group` read | Unit tests in resources.rs resolve `/CS` on both; a page-group fixture renders through `open_group` (asserted via its committed fingerprint changing when the page `/Group` is removed from the fixture) | S |
| 2 | Group-space compositing: `CmykA8` buffers, complemented separable blends, `convert` at the three boundaries | The fixture pairs' fingerprints are committed and asserted unequal; injection (composite in RGB regardless) turns the suite red; `Lab` group `/CS` yields the typed warning | M |
| 3 | `icc::Profile::parse` | **Done.** Header, tag table, `curv`/`para`/`XYZ` tags, ten typed refusals each reachable by a test; `icc_profile` fuzz target landed with it. Two changes from the plan: `chad` is not read (a matrix profile's columns are already adapted, so applying it again would adapt twice), and the two `bounds_ledger.rs` rows arrived later, on 4 September 2026, rather than with the parser — see the note below | M |
| 4 | `Transform` for matrix/TRC profiles | **Done.** Compiled 4 096-entry tables, an s15.16 matrix that is the profile's columns already multiplied by XYZ-to-sRGB, integer evaluation; `cargo xtask libm` passes. The sRGB round trip holds within one level, and grey profiles ride the same path with one curve | M |
| 5 | LUT profiles | **Done** for v2 mft1 and mft2, which a second census found to be 415 of the corpus 418 A2B tags — 409 mft2, 6 mft1, and 408 of them four channels to three. v4 mAB is three tags and refused by name. The grid is multilinear over its axes in integers, and both the v2 legacy Lab encoding and XYZ are read | L |
| 6 | `ColorSpace::Icc` wired | **Done**, except rendering intents, which stay parsed and discarded. No ICCBased fingerprint was committed and none was needed: no existing fixture names an `ICCBased` space, so nothing moved, and the facade tests assert pixels directly. The fallback is asserted rather than warned — see the note below | M |
| 7 | Known-answer tables | **Done**, in the form the arithmetic allows: the fixed-point encodings are pinned bit pattern by bit pattern, a linear curve's compiled ramp is pinned entry by entry across all 4 096, and two injections are counted (2 and 3 of 3 012) | S |
| 8 | Corpus movement | `corpus/report.json` shows the `iccbased` count; the metamorphic rows of [render-verification](render-verification.md) still hold over the files carrying profiles, so the new conversion path did not break resolution or rotation coherence | S |
| 9 | K-02.2: the scope's default substituted on every painting path that reads a space, and the three selections it rides on put right | `default_spaces.rs` (new): `each_default_space_draws_as_its_substitute_named_directly`, where each default — CalGray and ICCBased N=1, CalRGB and ICCBased N=3, ICCBased N=4 — through the operators, `cs`, a sampled image, an inline image, a shading, an `/Indexed` base, an uncoloured pattern's base and a selected `/Separation` alternate renders in light byte-identical to its twin naming the substitute, under the scope, group, `/BC`, initial-colour and refusal rules above; `the_selections_a_default_rides_on_read_what_the_file_names`, failing first; `an_ink_render_keeps_a_device_cmyk_colour_s_ink_under_default_cmyk`; fingerprints unchanged, and a default-space page added to `determinism.rs` | L — **Done**, 25 October 2026. A refusal's `reason` is `DefaultSpaceFamily`, `DefaultSpaceComponents` or `DefaultSpaceUnreadable`, noted when the scope's defaults are read, once a render. A form group's `/CS` is read where `Do` runs and remapped by the defaults of the scope its contents run in, so what it tolerates is reported where it always was |
| 10 | K-02.3: extraction reports the space the samples are in | `default_spaces.rs`: `an_extracted_image_names_the_space_its_samples_are_in` — the stated space kept, the substitute reported, the twin's `sample_space` the same, one XObject drawn in two spaces listed twice, a refusal named in `warnings`; `a_default_space_page_writes_the_svg_its_twin_writes` | M |
| 11 | K-02.4: the census, by scope | `default_space_census.rs` (new): `the_census_finds_a_default_in_every_scope` counts synthetic files carrying a default in each scope exactly, and a form a page's default does not reach; the `#[ignore]`d corpus test prints `default-space-census: RAN`, or `SKIPPED` with no corpora, and `corpus.yml`'s `Hold every corpus census` step names it | M |
| 12 | K-02.5: the census's first figures | The dispatched `corpus.yml` run on `develop` is green and its log carries `default-space-census: RAN`; the counts it printed are committed as floors in `default_space_census.rs` and quoted, with the run's id, in `docs/features/rendering.md` | S |

### Rendering intents, measured and declined

Scope bullet three and milestone 6 both ask for intents: `/Intent` on the
`ICCBased` stream, the `ri` operator, and ExtGState `/RenderingIntent`. Ruling
3 says a capability is built when the corpus asks for it, so the corpus was
asked:

| | files |
| --- | ---: |
| ExtGState `/RenderingIntent` | **0** |
| the `ri` operator | 5 |
| `/Intent` anywhere | 50, overwhelmingly 8.9.5.1's on an image |

Nothing sets an ExtGState intent, and an intent only changes an answer at all
for a LUT profile carrying more than one `A2B*` table. So what the corpus
actually asks for is that the **default** be right, and 8.6.5.8 makes that
`RelativeColorimetric` — which is `A2B1`, which is what the parser reaches for
first. Selecting a non-default intent is left unbuilt and said so here, rather
than built for five files that may not even exercise it.
*In scope since 9 October 2026*: the owner's parity decision makes every intent
[ROADMAP](../ROADMAP.md) row RC-01, and ruling 3 as amended uses this count to
order the row inside its tier rather than to decline it.

The wrinkle if it is ever wanted: an intent is graphics state and a transform
is compiled per colour space, so a space would need one compiled transform per
intent rather than one. That is the reason it is not a two-line change, and it
is worth knowing before someone estimates it at two lines.

### Three departures from the plan above, and why

**No `RenderWarning` for a refused profile.** Milestone 6 asked for one under
ruling 10. It is the wrong instrument here, and the census says why: 143 of
2 750 profiles are refused, overwhelmingly because they need the `A2B*` tables,
and they sit in 131 files that are otherwise fine. A warning fires per page and
would appear on documents whose colours are *unchanged from what this engine has
always produced* — the component-count reading is not a degradation from
anything, it is the status quo the profile could have improved on. Ruling 10
wants leniency reported so that "it opened" and "it opened cleanly" stay
distinguishable; a profile that could not be read makes no page less correct
than it was yesterday. The refusal is a typed `IccError` the caller matches on,
and the corpus number is the report.

**The two `bounds_ledger.rs` rows, landed 4 September 2026.** They were
declined when milestone 3 shipped, in these words: the ledger's contract is
heavier than a constant — each row publishes its figure in a markdown table in
the constant's own doc, names a test that fires it without a clock, and clears
three yardsticks — and claiming it without doing it would be the dressing-up
milestone 7 of the JBIG2 plan was corrected for. That was right, and the work
has since been done rather than the claim withdrawn.

`MAX_ICC_TAGS` and `MAX_ICC_BYTES` are rows 36 and 37 of forty. Both carry the
table, both name a dedicated `#[test]` in `icc.rs` — `Profile::parse`'s
refusals were already exercised, but inside **one** shared test among ten
assertions, and a bound whose proof is one assertion in a test named after
something else is a bound a rename can silently unprove. What the rows add
beyond the refusal is the ordering each cap depends on: the byte cap fires
before the `acsp` signature is read, and the tag cap before the `12 * count`
table it describes is walked, so a 132-byte profile declaring four billion tags
is refused rather than indexed into.

The five figures per row, measured rather than recalled:

| | `MAX_ICC_TAGS` | `MAX_ICC_BYTES` |
| --- | ---: | ---: |
| The most any fixture in this repository spends | 7 | 392 |
| A 200-page comic archive | 0 | 0 |
| A dense fixed document | 18 | 718 672 |
| A 300-page reflowable book | 0 | 0 |
| The cap | 1 024 | 2 MiB |

The two zeros are facts about those formats rather than absences of
measurement, and they have one cause: **nothing in this engine reads a PNG
`iCCP` chunk or a JPEG `APP2` profile**, so a comic page and a book picture
carry their profiles past this parser without them ever being opened. A fixed
document is different, and the reason is worth recording because it is not
obvious from `xps/profiles.rs`, which reads only §7.2's header through
`icc::data_space` and never calls `Profile::parse` at all: a `ContextColor`
profile part is embedded **verbatim** into the synthesised PDF as an
`/ICCBased` space, and it is *rendering that document* — `Resources`'s
`ICCBased` arm — that hands it to the parser. So the XPS route reaches these
caps by the ordinary PDF road, one pass later than it looks.

The document column's two numbers are the corpus's own, over every `acsp`
stream in all 5 594 files: the largest profile is **718 672 bytes** and the
busiest declares **eighteen** tags. The margins are 2.9x and 57x. Eighteen
corrects the "declares seventeen" this document and the constant both carried;
the census that produced seventeen counted profiles the parser *accepts*, and a
cap is spent by profiles it is handed.

**`chad` is not read.** The plan lists it. A matrix profile's `rXYZ`/`gXYZ`/
`bXYZ` columns are already relative to the D50 connection space — that is what
makes them addable — so applying the chromatic-adaptation tag on top would
adapt a second time and shift every colour. It is read by profiles that need to
recover the *unadapted* primaries, which nothing here does.

## Dependencies

- `tinker-pdf-math` as it stands (`pow`, `cbrt`, `ln` — no new
  transcendentals needed) and the `cargo xtask libm` gate over
  `PIXEL_PATHS`.
- The `parse_space` / `parse_function` resource seam in
  `crates/tinker-pdf/src/resources.rs`; the `open_group` machinery and
  budgets in `crates/tinker-pdf-render/src/lib.rs` (reused, not forked).
- The fingerprint suite (`crates/tinker-pdf/tests/determinism.rs`) and the
  bounds ledger (`crates/tinker-pdf/tests/bounds_ledger.rs`).
- [render-verification](render-verification.md)'s metamorphic rows —
  milestone 8 only; nothing earlier waits on it.
- [H-135](../roadmap/H.md), the owner's open question, for milestone 9:
  it builds H-135's default — an ink render keeps a `k` colour's own
  components under `/DefaultCMYK` — until the owner decides. Milestone 12's
  corpus run waits on MS-11 and H-03, as K-02.5 lists them.

## Risks

| Risk | Mitigation |
| --- | --- |
| A platform transcendental sneaks into per-pixel evaluation and rendering diverges across targets | Evaluation is integer/fixed-point by construction; `cargo xtask libm` fails the build on any transcendental in `tinker-pdf-color`; the committed fingerprints are measured on all four targets |
| Hostile profiles: absurd tag counts, CLUT grids sized to exhaust memory, overlapping tag data | Ruling 1 discipline — the `icc_profile` fuzz target lands with the parser; profile size and tag count are measured rows in `bounds_ledger.rs` and CLUT volume is a named refusal without one; over-bounds is a typed refusal to `Approximated`, never an allocation |
| CMYK group buffers (5 bytes/pixel) inflate memory on group-heavy pages | Buffers are already bounded to the group's device-space extent and `MAX_GROUP_BUFFERS` caps the count; the existing decline-and-warn path (`GroupBudgetSpent`) absorbs the excess |
| Non-separable blends in CMYK groups are an approximation and a reviewer mistakes it for a bug | The conversion is a named, warned limit on the type and in this doc; the fixture for it commits the approximated fingerprint so any silent change is caught |
| Every colour-touching fingerprint churns when the CMM lands | Expected and staged: milestone 6 re-records fingerprints in its own PR with before/after renders attached, the discipline `corpus/ratchet.json` updates already follow |
| **No second CMM adjudicates this, under ruling 13.** A misreading of ICC.1 that survives one review converts every profiled page wrongly, and every check here agrees with it | Matrix/TRC transforms are pinned bit-exactly against tables hand-computed from named clauses, which is arithmetic rather than opinion. LUT interpolation has no such anchor and the doc says so; it is the largest named limit of this design |
| v2 versus v4 profile differences (PCS encoding, curve types) handled subtly wrongly | Known-answer tables per profile version in milestones 4, 5 and 7, each citing the clause it came from, over both v2 and v4 fixture profiles |
| A silence the default-space design decides — a form group's scope, the initial colour after `cs`, a default's own spaces, a refused default — is later read the other way | Each is stated as this design's decision rather than the standard's, and a K-02.2 test holds it, so a different reading moves one test and one function |
| A page's default stops at a form with `/Resources` of its own, where a reader that carries defaults inward draws the substitute | The clause's words are followed ("the current resource dictionary"; 8.10.1); K-02.4 counts the files where the two readings part, and K-02.5 records the count |
| The device operators, the most frequent in a content stream, start paying for the seam | A scope's defaults are read once and the interpreter branches on a field it holds; K-02.2 times release and debug and puts any cost to the owner |
| H-135 is decided for conversion | One change to `resolve_ink`; K-02.2's ink test names the contract it holds, so the change shows where it lands |
| A twin comparison over a JPEG passes with nothing applied and is taken for proof | K-02.2's twins use sampled images, and the JPEG path is a named non-goal |
