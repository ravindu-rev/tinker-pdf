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
records is that corpus pages with `ICCBased` spaces move under the parity
budget of [render-parity](render-parity.md).

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
- Fixtures, fingerprints, a fuzz target, and a subprocess oracle for all of
  the above ([verification.md](../verification.md)).

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
space). In stage 1 the device relations already in `ColorSpace::to_rgb`
supply the forward direction and their stated inverses (RGB→CMYK with
`k = min(1-r, 1-g, 1-b)` undercolour removal) supply the backward one,
named on the type as approximations; stage 2 replaces both ends with
profile transforms without moving the call sites. Soft-mask luminosity
(11.6.5.2) reads the group's own space, which fixes `/BC` handling for
CMYK mask groups for free.

**Proof.** New fixtures in `crates/tinker-pdf/tests/determinism.rs`, in
pairs that differ only in the group's `/CS` — the same two colours
multiplied inside an RGB group and inside a CMYK group, an isolated and a
non-isolated variant, and one page-level `/Group` file. Each pair's two
fingerprints are committed and asserted *unequal*, which is the injection
test built into the fixture: revert to compositing in RGB and the pair
renders identical, so the suite goes red.

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
- **Oracle, the `opj_decompress` pattern:** Little CMS's `transicc` (with
  Argyll's `xicclu` as the named alternate) is a CLI that applies a
  profile to component values on stdin — invoked as a subprocess in a new
  `crates/tinker-pdf-color/tests/icc_oracle.rs`, never linked (ruling 9).
  For each fixture profile and a grid of inputs, our transform must agree
  within a per-channel budget of 2/255 — budgeted, not bit-exact, because
  CMMs legitimately interpolate differently. The test prints
  `icc-oracle: RAN <version>` / `SKIPPED` and CI greps it red on
  `SKIPPED`, exactly as `qpdf-oracle:` does (ruling 9).
- **Corpus evidence:** the `tpdf probe` capability scan
  (`scan_capabilities` in tools/tpdf/src/main.rs) grows an `iccbased` tag beside `jbig2` and
  `jpx`, so `corpus/report.json` measures how many real files carry
  profiles, and *which kinds* — the ruling-3 evidence that schedules the
  LUT milestone, and the selector that puts these files into the
  [render-parity](render-parity.md) manifest.
- **Fingerprints:** the stage-1 pairs plus new ICCBased fixtures are
  committed fingerprints in `determinism.rs`, holding ruling 4's
  bit-identical contract across targets once profiles drive conversion.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size (S/M/L/XL) |
| --- | --- | --- | --- |
| 1 | `Group.space` resolved: form `/Group /CS` and page-level `/Group` read | Unit tests in resources.rs resolve `/CS` on both; a page-group fixture renders through `open_group` (asserted via its committed fingerprint changing when the page `/Group` is removed from the fixture) | S |
| 2 | Group-space compositing: `CmykA8` buffers, complemented separable blends, `convert` at the three boundaries | The fixture pairs' fingerprints are committed and asserted unequal; injection (composite in RGB regardless) turns the suite red; `Lab` group `/CS` yields the typed warning | M |
| 3 | `icc::Profile::parse`: header, tag table, `curv`/`para`/`wtpt`/`chad`/matrix tags, typed refusals | `icc_profile` fuzz target in the same PR runs clean on its seed corpus; malformed-profile tests hit each refusal variant; two bounds rows added to `bounds_ledger.rs` | M |
| 4 | `Transform` for matrix/TRC profiles: compiled tables, s15.16 matrix, integer eval | `cargo xtask libm` passes with the new code in place; round-trip tests (sRGB profile → PCS → sRGB identity within 1/255); fixed known-answer tests for a committed test profile | M |
| 5 | LUT profiles: `mft1`, `mft2`, `mAB `, `mBA `, integer CLUT interpolation | Known-answer tests against hand-computed CLUT lookups; fuzz corpus extended with LUT profiles, still clean | L |
| 6 | `ColorSpace::Icc` wired: `ICCBased` arm, fallback warning, `iccbased` probe tag, rendering intents | New ICCBased fingerprints committed; a refused profile produces the ruling-10 warning in `Bitmap.warnings` (asserted); `corpus/report.json` shows the `iccbased` count after a nightly run | M |
| 7 | `icc-oracle` subprocess test | CI log contains `icc-oracle: RAN`; removing `transicc` from `PATH` in a scratch run goes red on the `SKIPPED` grep; grid agreement within 2/255 per channel on every fixture profile | S |
| 8 | Parity movement | The render-parity manifest includes the `iccbased`-tagged corpus files and [render-parity](render-parity.md)'s `parity-run --check` holds them under budget; the ratchet update recording the improvement lands in the same PR | S |

## Dependencies

- `tinker-pdf-math` as it stands (`pow`, `cbrt`, `ln` — no new
  transcendentals needed) and the `cargo xtask libm` gate over
  `PIXEL_PATHS`.
- The `parse_space` / `parse_function` resource seam in
  `crates/tinker-pdf/src/resources.rs`; the `open_group` machinery and
  budgets in `crates/tinker-pdf-render/src/lib.rs` (reused, not forked).
- The fingerprint suite (`crates/tinker-pdf/tests/determinism.rs`) and the
  bounds ledger (`crates/tinker-pdf/tests/bounds_ledger.rs`).
- [render-parity](render-parity.md)'s manifest and ratchet — milestone 8
  only; nothing earlier waits on it.
- One pinned oracle package (`transicc` or `xicclu`) installed in CI, as
  `opj_decompress` already is.

## Risks

| Risk | Mitigation |
| --- | --- |
| A platform transcendental sneaks into per-pixel evaluation and rendering diverges across targets | Evaluation is integer/fixed-point by construction; `cargo xtask libm` fails the build on any transcendental in `tinker-pdf-color`; the committed fingerprints are measured on all four targets |
| Hostile profiles: absurd tag counts, CLUT grids sized to exhaust memory, overlapping tag data | Ruling 1 discipline — the `icc_profile` fuzz target lands with the parser; profile size and CLUT volume are measured rows in `bounds_ledger.rs`; over-bounds is a typed refusal to `Approximated`, never an allocation |
| CMYK group buffers (5 bytes/pixel) inflate memory on group-heavy pages | Buffers are already bounded to the group's device-space extent and `MAX_GROUP_BUFFERS` caps the count; the existing decline-and-warn path (`GroupBudgetSpent`) absorbs the excess |
| Non-separable blends in CMYK groups are an approximation and a reviewer mistakes it for a bug | The conversion is a named, warned limit on the type and in this doc; the fixture for it commits the approximated fingerprint so any silent change is caught |
| Every colour-touching fingerprint churns when the CMM lands | Expected and staged: milestone 6 re-records fingerprints in its own PR with before/after renders attached, the discipline `corpus/ratchet.json` updates already follow |
| The oracle CMM disagrees legitimately (interpolation, intent details) and the budget flaps | Agreement is budgeted at 2/255 and the package version is pinned; a pin bump re-records in the same PR, the render-parity rule |
| v2 versus v4 profile differences (PCS encoding, curve types) handled subtly wrongly | Known-answer tests per profile version in milestone 4/5; the oracle grid runs over both v2 and v4 fixture profiles |
