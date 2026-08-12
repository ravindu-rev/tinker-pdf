# Gap plans

One document per outstanding item. Each is self-contained: read it cold and it
tells you what breaks today, what to build, what to reuse and where, and how
you know when it is done.

These are not phases. A phase is a body of work with a checkpoint; several of
these are a two-line fix and a test. They live here rather than in the phase
numbering so that neither set buries the other.

Where a gap document re-specifies something a phase plan already got right —
plan 07's image-sampling matrix, plan 08's optional-content seam, plan 05's
CMap build step — it cites and does not restate. The phase plans remain the
architecture; these say what is missing from it.

Sizes are the project's usual bands: S ≈ 0.5 engine-months, M ≈ 1–2, L ≈ 2–4,
XL ≈ 5–8 ([PLAN.md](../../PLAN.md)).

## Fonts — glyph selection is wrong, not missing

| # | Plan | What goes wrong today | Size |
| --- | --- | --- | --- |
| 01 | [CFF glyph selection](01-cff-glyph-selection.md) | ~~The character code is used as the glyph index. Code 65 fetches GID 65~~ **DONE**, see the plan's `As built`: charset formats 0/1/2 and the three predefined ones, the string INDEX and the 391 standard strings, the built-in encoding with supplements, `ROS`/FDArray/FDSelect with per-FD private dicts and matrices, and the whole four-step fallback chain wired into all three sites that used the code as an index. The plan's symptom description was corrected: a subset font drew *nothing*, and only a full one drew the wrong letter | L |
| 02 | [CID to GID](02-cid-to-gid.md) | ~~The CID is computed for widths and not for outlines. Wrong glyphs at correct spacing~~ **DONE**, see the plan's `As built`: `DecodedCode.cid`, `/CIDToGIDMap` in both forms with an out-of-range CID reported as `.notdef`, and the CID deciding the glyph for a composite font. The CIDFontType0 half turned out to be owed nothing — 01 had finished it — and the TrueType half had a second symptom the plan does not describe: the code reached the font's own `cmap` through `/ToUnicode`, which answers a question a composite font never asks | M |
| 03 | [Predefined CMaps](03-predefined-cmaps.md) | ~~Only `Identity-H`/`-V` are real; the rest are stubs that mis-split *and* mis-map~~ **DONE**, see the plan's `As built`: all 202 CMaps of Adobe's registry vendored and compiled by the workspace's first `build.rs` into 1.19 MB of deflated tables, the registry's own `usecmap` followed, a `cmap-predefined` feature whose measured wasm delta is recorded in plan 05, and a named warning for anything outside the set. Two defects the plan does not describe, both in `decode_codes`: 9.7.6.2's codespace bounds are per *byte* and were compared as one integer interval — which is the only thing separating GB18030's two-byte and four-byte codes — and 9.7.6.3 gives an undefined code its length from its lead byte, where the code consumed one byte and re-read a trail byte as a lead. The subset question the non-goals left open was answered by measurement rather than by guess: the whole registry costs 1.19 MB, so all of it ships | L |
| 04 | [usecmap and codespaces](04-usecmap-and-codespaces.md) | ~~An embedded CMap that inherits from a parent gets only what it declares inline~~ **DONE**, see the plan's `As built`: `usecmap` and `/UseCMap` in all three forms, merged so the child overrides the parent where they overlap, capped at four links with a cycle guard keyed on sources rather than names, and `end*` matched by name. Two things the plan does not describe: a truncated section also *swallowed* the keyword that ended it, and `CMap::cid` tested its identity flag ahead of its own entries, which would have thrown a child's `cidrange` away the moment it inherited from any CJK stub. The parents in every test are hand-built — 03 is what makes a real one | S |
| 05 | [Vertical metrics](05-vertical-metrics.md) | ~~Vertical text advances by horizontal widths~~ **DONE**, see the plan's `As built`: `/W2` in both of Table 117's forms and `/DW2` with its `[880 -1000]` default, a `vertical_metrics(cid)` accessor returning all three components, 9.4.4's `ty` applied to the pen and the position vector applied to the glyph's placement. Three defects the plan does not describe, all in the same branch: `TJ` adjustments moved a vertical pen *sideways* (9.4.3 puts them on the coordinate the writing mode names, so a kerned column drifted out of itself); `Th` was applied to the vertical advance, which 9.4.4's `ty` does not carry; and `Glyph::advance` had no stated sign, which is the field a consumer would have read a column's direction from. **This closes the font lane** | M |

## Rendering — capabilities that exist and are wrong

| # | Plan | What goes wrong today | Size |
| --- | --- | --- | --- |
| 06 | [Optional content](06-optional-content.md) | A layer marked `/OFF` paints at full strength, with no warning | M |
| 07 | [Stroked patterns](07-stroked-patterns.md) | ~~A gradient-stroked rule draws solid black, silently~~ **DONE**, see the plan's `As built`: all four milestones together, because the hazard the plan names is the shading half landing without the tiling warning. `stroke_path` and the stroking half of `show_glyph` fill the outline `stroke()` already computes through the existing `fill_with_pattern`, so no pattern machinery was added and the `UnsupportedPattern` path is shared rather than duplicated. Three things the plan does not describe: the `scn` components were dropped for the **fill** slot too, not only the stroke slot; `[/Pattern base]` (8.7.3.2) did not parse at all, so those components could not have been read in any space; and `fill_with_pattern` applied `ca` to everything, which is wrong for a stroke. The **filling** half of `show_glyph` had the same defect and is fixed with it — a `2 Tr` glyph would otherwise have painted a black body under a patterned edge. Adds the repository's fifth determinism fingerprint, because `fill_with_pattern` had none. Fourteen tests; injection shows the anchoring test and the fill-rule test are each the **only** assertion that catches their defect | S |
| 08 | [Inline image filters](08-inline-image-filters.md) | A Flate-with-predictor inline image decodes to noise | S |

## Rendering — features never built

| # | Plan | What goes wrong today | Size |
| --- | --- | --- | --- |
| 09 | [Tiling patterns](09-tiling-patterns.md) | Reported, not painted | M |
| 10 | [Mesh shadings](10-mesh-shadings.md) | Types 4–7 warn and skip | M |
| 11 | [Transparency groups](11-transparency-groups.md) | `/Group` and ExtGState `/SMask` are not read at all | L |

## Rasteriser — plan 07 says "complete" and five scope items are absent

| # | Plan | What goes wrong today | Size |
| --- | --- | --- | --- |
| 12 | [Image sampling](12-image-sampling.md) | Nearest-neighbour only, in the wrong crate, `/Interpolate` never read | M |
| 13 | [Quadratic path verb](13-quadratic-path-verb.md) | TrueType quadratics are up-converted to cubics | S |
| 14 | [Bounded painting](14-bounded-painting.md) | Every paint is O(canvas). One full-page mask per glyph | M |
| 15 | [Cancellation](15-cancellation.md) | One `fill()` cannot be interrupted, whatever its size | S |

## Filters

| # | Plan | What goes wrong today | Size |
| --- | --- | --- | --- |
| 16 | [CCITT completion](16-ccitt-completion.md) | Two parameters ignored, `/K > 0` is not mixed mode, output is 8× too wide | M |
| 17 | [JBIG2 generic region](17-jbig2-generic-region.md) | Refused. The refusal path is the feature | L |
| 18 | [JPX: the decision](18-jpx-decision.md) | Refused. Costed honestly so it can be decided rather than drifted into | S |

## Writing and semantics

| # | Plan | What goes wrong today | Size |
| --- | --- | --- | --- |
| 19 | [Encrypt and linearize](19-encrypt-and-linearize.md) | Linearization is silently dropped when encryption is on | M |
| 20 | [Linearization validation](20-linearization-validation.md) | The hint tables have never been checked by anything | S |
| 21 | [Metadata: absent versus empty](21-metadata-absent-vs-empty.md) | ~~A title that is present and blank reads as absent~~ **DONE**, see the plan's `As built` | S |
| 22 | [PDF version and /Trapped](22-pdf-version-and-trapped.md) | ~~The catalog overrides the header unconditionally; `/Trapped` is absent~~ **DONE**, see the plan's `As built`: the later of the two wins, compared numerically; an unreadable header reports the 1.7 baseline with a `HeaderMissing` warning; `/Trapped` reads as its three names | S |

## Test infrastructure and shipping

| # | Plan | What goes wrong today | Size |
| --- | --- | --- | --- |
| 23 | [Corpus runner](23-corpus-runner.md) | No corpus has ever been run. Eight real files, total | M |
| 24 | [Fuzz execution](24-fuzz-execution.md) | ~~Eleven targets compile and none has ever run~~ **M1–M4 done**, see the plan's `As built`: fifteen targets, seed corpora, a nightly job and a per-PR replay. M5, the first real campaign, is outstanding — the longest run so far is thirty seconds a target | S |
| 25 | [The wasm determinism leg](25-wasm-determinism-leg.md) | ~~Three targets prove ruling 4; the fourth job has never executed~~ **M1–M3 done**, see the plan's `As built`: it executes, and `wasm32-wasip1` reproduces all four fingerprints byte-for-byte against native Windows. That is 2 of 4 targets on one machine — no CI run has been observed, so linux and macos are still only claimed. M4, fixture growth, is owed by 09, 10, 11 and 12 | S |
| 26 | [Binding packaging](26-binding-packaging.md) | Four surfaces build and none ships | M |

## Decisions, not implementations

| # | Plan | The question |
| --- | --- | --- |
| 27 | [Form calculations](27-form-calculations-decision.md) | Hand-rolled ES subset, data-only, or nothing |
| 28 | [Tinker integration decisions](28-tinker-integration-decisions.md) | EPUB/XPS/CBZ, the licence, and a parity blocker in no plan |

## Ordering

[16-build-sequence.md](../16-build-sequence.md) ranks the whole set by value
over risk. Four constraints inside it are hard rather than advisory:

- **11 before 09.** Tiling patterns need `Canvas::composite` and a rasterised
  tile. Without it the honest implementation is pathologically slow and the
  fast one is wrong.
- **23 before 10 and 17.** Ruling 3 schedules deferred capabilities by
  corpus hit-rate. Building either on judgement is building against no
  evidence.
- **12–15 before any golden re-baseline.** They all move pixels. Re-baselining
  once, at the end, is the difference between one reviewed change and six that
  each hide the next.
- **20 before claiming linearization works.** The layout is checked against
  its own bytes, which cannot catch a wrong hint table.
