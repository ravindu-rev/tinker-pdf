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
[roadmap](../ROADMAP.md) names both gaps and points them here; the exit it
records is that ICC profiles drive conversion and that known-answer tables
computed from the specification's own equations hold
([render-verification](render-verification.md)).

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

## Non-goals

- **Output intents and proofing** (14.11.5): `/OutputIntents` handling
  belongs to [pdfa](pdfa.md); this design converts for the screen, to
  sRGB, always.
- **N-channel output, overprint and spot-ink simulation.** `Separation` /
  `DeviceN` keep flowing through their tint transforms (8.6.6.4, 8.6.6.5);
  simulating ink interaction is a different renderer.
- **Black point compensation, gamut mapping beyond intent selection,
  iccMAX/v5 profiles, named-colour tags.** Refused by name, per ruling 2.
- **Profiles embedded inside image codecs.** JPX colour boxes already have
  their own path in `tinker-pdf-filters`; routing them through this CMM is
  scheduled by corpus evidence (ruling 3), not assumed here.
- **Exact CalGray/CalRGB.** `parse_space` maps them to device spaces
  today; 8.6.5.2–8.6.5.3 fidelity rides on the same PCS machinery and can
  follow, but no milestone below depends on it.

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
files. The largest profile in the corpus is **718 672 bytes**, which is the
figure the profile-size bound in `bounds_ledger.rs` has to clear.

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
inverse — a rich black — comes back as its pure-K equivalent. Nothing in this
engine authors CMYK components (`resolve_color` flattens every source colour to
sRGB at the resource seam), so every value in a group buffer originated from
the inverse and round-trips. The day components cross that seam, this is the
paragraph to revisit. Soft-mask luminosity (11.6.5.2) reads the group's
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
`crates/tinker-pdf/tests/bounds_ledger.rs`: maximum accepted profile size
and maximum CLUT grid volume, measured against real embedded profiles.

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

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size (S/M/L/XL) |
| --- | --- | --- | --- |
| 1 | `Group.space` resolved: form `/Group /CS` and page-level `/Group` read | Unit tests in resources.rs resolve `/CS` on both; a page-group fixture renders through `open_group` (asserted via its committed fingerprint changing when the page `/Group` is removed from the fixture) | S |
| 2 | Group-space compositing: `CmykA8` buffers, complemented separable blends, `convert` at the three boundaries | The fixture pairs' fingerprints are committed and asserted unequal; injection (composite in RGB regardless) turns the suite red; `Lab` group `/CS` yields the typed warning | M |
| 3 | `icc::Profile::parse` | **Done.** Header, tag table, `curv`/`para`/`XYZ` tags, ten typed refusals each reachable by a test; `icc_profile` fuzz target landed with it. Two changes from the plan: `chad` is not read (a matrix profile's columns are already adapted, so applying it again would adapt twice), and the two `bounds_ledger.rs` rows are **not** added — see the note below | M |
| 4 | `Transform` for matrix/TRC profiles | **Done.** Compiled 4 096-entry tables, an s15.16 matrix that is the profile's columns already multiplied by XYZ-to-sRGB, integer evaluation; `cargo xtask libm` passes. The sRGB round trip holds within one level, and grey profiles ride the same path with one curve | M |
| 5 | LUT profiles: `mft1`, `mft2`, `mAB `, `mBA `, integer CLUT interpolation | Known-answer tests against hand-computed CLUT lookups; fuzz corpus extended with LUT profiles, still clean | L |
| 6 | `ColorSpace::Icc` wired | **Done**, except rendering intents, which stay parsed and discarded. No ICCBased fingerprint was committed and none was needed: no existing fixture names an `ICCBased` space, so nothing moved, and the facade tests assert pixels directly. The fallback is asserted rather than warned — see the note below | M |
| 7 | Known-answer tables | **Done**, in the form the arithmetic allows: the fixed-point encodings are pinned bit pattern by bit pattern, a linear curve's compiled ramp is pinned entry by entry across all 4 096, and two injections are counted (2 and 3 of 3 012) | S |
| 8 | Corpus movement | `corpus/report.json` shows the `iccbased` count; the metamorphic rows of [render-verification](render-verification.md) still hold over the files carrying profiles, so the new conversion path did not break resolution or rotation coherence | S |

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

**No `bounds_ledger.rs` rows.** `MAX_ICC_TAGS` and `MAX_ICC_BYTES` exist and
fire, but the ledger's contract is heavier than a constant: each row publishes
its figure in a markdown table in the constant's own doc, names a test that
fires it without a clock, and clears three yardsticks. `MAX_ICC_BYTES` clears
the corpus's largest profile (718 672 bytes) by a factor of nearly three, which
is the measurement that matters and is recorded above; the ledger rows are a
separate piece of work with its own discipline, and claiming them here without
doing it would be the dressing-up milestone 7 of the JBIG2 plan was corrected
for.

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

## Risks

| Risk | Mitigation |
| --- | --- |
| A platform transcendental sneaks into per-pixel evaluation and rendering diverges across targets | Evaluation is integer/fixed-point by construction; `cargo xtask libm` fails the build on any transcendental in `tinker-pdf-color`; the committed fingerprints are measured on all four targets |
| Hostile profiles: absurd tag counts, CLUT grids sized to exhaust memory, overlapping tag data | Ruling 1 discipline — the `icc_profile` fuzz target lands with the parser; profile size and CLUT volume are measured rows in `bounds_ledger.rs`; over-bounds is a typed refusal to `Approximated`, never an allocation |
| CMYK group buffers (5 bytes/pixel) inflate memory on group-heavy pages | Buffers are already bounded to the group's device-space extent and `MAX_GROUP_BUFFERS` caps the count; the existing decline-and-warn path (`GroupBudgetSpent`) absorbs the excess |
| Non-separable blends in CMYK groups are an approximation and a reviewer mistakes it for a bug | The conversion is a named, warned limit on the type and in this doc; the fixture for it commits the approximated fingerprint so any silent change is caught |
| Every colour-touching fingerprint churns when the CMM lands | Expected and staged: milestone 6 re-records fingerprints in its own PR with before/after renders attached, the discipline `corpus/ratchet.json` updates already follow |
| **No second CMM adjudicates this, under ruling 13.** A misreading of ICC.1 that survives one review converts every profiled page wrongly, and every check here agrees with it | Matrix/TRC transforms are pinned bit-exactly against tables hand-computed from named clauses, which is arithmetic rather than opinion. LUT interpolation has no such anchor and the doc says so; it is the largest named limit of this design |
| v2 versus v4 profile differences (PCS encoding, curve types) handled subtly wrongly | Known-answer tables per profile version in milestones 4, 5 and 7, each citing the clause it came from, over both v2 and v4 fixture profiles |
