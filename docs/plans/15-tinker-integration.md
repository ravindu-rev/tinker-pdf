# Phase 15 — Tinker integration

The final phase, and the only one that happens in the Tinker repository. When
it is done, Tinker runs on tinker-pdf, MuPDF and every workaround it forced
are deleted from Tinker's tree, and the limitations document that started this
whole project is historical. The precondition is absolute: **Checkpoint B
holds** — `tinker_parity.rs` green, corpus render parity ratcheted ≥ 95%,
write round-trips validated ([PLAN.md](../PLAN.md)). Integration is a
mechanical phase by design; every judgment call was moved earlier so this one
is mostly deletion.

## Scope

- Rewrite `crates/tinker-core/src/engine/{mod,render,text,outline}.rs`
  against the tinker-pdf facade.
- Golden regeneration and fixture handling.
- The deletion checklist — every MuPDF trace in Tinker's tree.
- Three owner decisions that only become decidable now, recorded with their
  options.

## Non-goals

- An engine trait. Tinker's standing architecture ruling ("no engine trait
  yet — mupdf's types never cross tinker-core's public API, preserving the
  seam for free") holds: the swap happens *inside* the `engine` module,
  DTOs in `dto.rs` unchanged, and no abstraction layer is introduced for a
  migration that happens once.
- A long-term dual-engine mode. A `legacy-mupdf` cargo feature keeps the old
  path buildable **during the integration PR series only**, for A/B
  differential runs on real documents; it is deleted before the series
  merges. Two engines forever is two sets of bugs forever.
- Editing/forms/creation integration — those engine phases
  ([10](10-editing.md)–[12](12-creation.md)) land after this one at their own
  pace; this phase swaps what Tinker uses today: open/auth, metadata,
  permissions, geometry, outline, text, search, render.

## Design

### The swap

The current engine module is small and fully mapped (four files; open/auth,
page count, metadata, permissions, geometry, outline, text+search, render
full+tile). Each function body is rewritten from `mupdf::*` calls to facade
calls; signatures and DTOs stay put, so nothing outside
`crates/tinker-core/src/engine/` — not the actor, not the registry, not the
Tauri commands, not the CLI — changes in the swap commits. The one error-type
coupling outside the module (`impl From<mupdf::Error> for TinkerError` in
`error.rs`) is replaced by the equivalent for tinker-pdf's error enum, keeping
the `ENGINE_ERROR` code contract.

Simplifications the swap claims immediately, each traceable to a documented
MuPDF limitation:

- `finish_open` reports **real** `AuthLevel::Owner` — the "cannot tell owner
  from user" caveat and the sys-call plan in Tinker `plans/10` M0 are
  deleted, not implemented.
- `open_bytes`'s `magic` parameter dies; the engine sniffs.
- The `Renderer` display-list LRU is deleted; tinker-pdf caches decoded
  content and glyph rasters internally, and the `invalidate()` dead-code hook
  goes with it.
- The actor model **stays** — it is Tinker's serialization boundary for
  mutations and its message API is good — but it stops being load-bearing
  for reads: the render-clone-pool design in Tinker `plans/01` (a workaround
  for `!Send` contexts) is replaced by plain concurrent renders on the
  `Send + Sync` document.
- `TextLine.rtl` is populated compatibly from the new separate
  `wmode`/`bidi` fields (Tinker's current field conflates them; the DTO
  keeps its name until Tinker's own text-layer work renames it).

### Goldens and fixtures

Every visual golden changes — a different rasterizer is a different image.
The gate is not "matches MuPDF" but "as good": `TINKER_UPDATE_GOLDENS=1`
regenerates, `pdfcmp` produces side-by-side + heat-map artifacts, a human
reviews them once, and the new goldens become law under the same thresholds
(the metric shapes match by construction — [14](14-testing-and-corpora.md)).
The four fixtures: Tinker's `gen-fixtures.rs` example is MuPDF's last stand;
it is deleted with the dependency, and the fixtures are frozen as committed
binaries with a provenance README — a documented exception to Tinker's
no-committed-binaries rule — until engine phase [12](12-creation.md)
self-hosts generation and the exception is lifted.

### The deletion checklist

Exact paths, verified against Tinker's tree at planning time, and
**re-verified 16 August 2026 against Tinker at `f33ce8a`** — the verification
is recorded item by item below rather than in an amendment at the foot of the
file, because a checklist is read where it is used.

**Twenty-two named items. All twenty-two still exist, at the paths stated.
Nothing has moved.** The freeze held for everything the checklist covers: the
two commits since the scaffolding commit (`a89d7c0`, `f33ce8a`) adopted this
engine as a submodule and advanced its pointer, and touched none of the MuPDF
surface. What the re-check found instead is five traces the checklist does
*not* name, listed after it.

- `third_party/mupdf-msvc/` (the vendored patched wrapper, both patches).
  **Present**; 60 vendored files, and `PATCH.md` still documents exactly two
  patches — the MSVC `max_align_t` fix and `permissions()`
  `from_bits_truncate`.
- `scripts/vendor-mupdf-patch.mjs`, `scripts/check-mupdf-deps.mjs`, the
  `check:mupdf` entry in `package.json`, and the "MuPDF dependency
  discipline" CI step. **All four present**; the CI step is `ci.yml:27`.
- `Cargo.toml`: the `[workspace.dependencies] mupdf` block,
  `[patch.crates-io] mupdf`, `[profile.dev.package.mupdf-sys]`. **All three
  present**, unchanged, including the ten features the block enables — of
  which `xps`, `cbz` and `epub` are decision 1's subject, and `base14-fonts`
  and `system-fonts` are what supplies faces today to documents that embed
  none — the thing a `FontProvider` has to replace.
- `.cargo/config.toml`: `MUPDF_MSVC_PLATFORM_TOOLSET`. **Present**, `= "v143"`.
- `deny.toml`: `AGPL-3.0` in the allowlist, both `mupdf`/`mupdf-sys`
  exceptions. **All three present.** Decision 3 makes this file's whole
  premise — "Tinker is AGPL-3.0-or-later and links MuPDF" — historical, so it
  is rewritten rather than edited.
- CI: clang install steps in `ci.yml`; in `release.yml` the 90-minute
  timeouts, per-arch native macOS runner justification, and the
  source-archive check for `third_party/mupdf-msvc/Cargo.toml`. **All
  present**, with one correction: there is exactly **one** 90-minute timeout
  (`release.yml:142`), not several. The clang installs are two, at `ci.yml:39`
  and `ci.yml:84`.
- `CONTRIBUTING.md`: the C-toolchain prerequisites and both Windows build
  quirks; `.gitattributes` comments citing the vendored wrapper. **All
  present**; the quirks are still a section headed "Windows: the two build
  quirks", and `.gitattributes` cites the wrapper twice — the `eol=lf`
  rationale and `third_party/** linguist-vendored`.
- `docs/mupdf-limitations.md`: gains a historical banner ("resolved by
  tinker-pdf; kept as the record of why"); `docs/upstream/` bug drafts
  close with pointers. **Both present**; `docs/upstream/` holds exactly one
  draft, `mupdf-rs-permissions-bug.md`.
- The mobile CI jobs lose their "MuPDF cross-compilation is the riskiest
  item" rationale — pure Rust cross-compiles with rustup targets; the jobs
  stay, their risk register shrinks. **Present**, `ci.yml:97`.

#### Five traces the checklist above does not name

Found by the re-verification, and each would survive a checklist followed
exactly as written.

1. **`crates/tinker-core::engine_version()` returns `"mupdf-rs 0.8.0"`** from
   `lib.rs`, and it is *public*. The design section above says there is one
   error-type coupling outside the engine module; this is a second, it is
   surfaced to the user through `caps_get`'s `engine` field and `tinker
   doctor`, and a swap that misses it ships an app reporting the engine it no
   longer runs on.
2. **`crates/tinker-core/Cargo.toml` declares `mupdf = { workspace = true }`.**
   Deleting only the `[workspace.dependencies]` block leaves this inherit
   dangling and the workspace stops resolving — which is a loud failure rather
   than a silent one, but it is a second file, and `check-mupdf-deps.mjs`
   exists precisely because per-crate `mupdf` declarations are the thing that
   goes wrong here.
3. **Two CI settings are justified by MuPDF and outlive it**: `ci.yml`'s
   `CARGO_INCREMENTAL: 0` ("building MuPDF from source is slow and identical
   across jobs") and `release.yml`'s `cache-on-failure: true` ("a macOS job
   that dies after twenty minutes of MuPDF compilation"). Both settings may
   well be worth keeping; both comments become false.
4. **The source-archive job asserts something that is no longer true.** Its
   comment reads "`third_party/mupdf-msvc` IS tracked and is a plain directory
   *rather than a submodule*, so the vendored MuPDF patch travels with the
   archive". Tinker has since gained a real submodule at `engine/tinker-pdf`
   (`a89d7c0`), and `git archive` does not follow submodules — so once the
   swap lands, the released source archive ships a workspace that cannot
   build, and "Verify what went in" checks for `third_party/mupdf-msvc` rather
   than for the engine. This is the one drift finding with a shipped
   consequence, and it belongs to 15.4 rather than 15.3.
5. **Milestone 15.3's exit criterion cannot go green as written.** `rg -i
   mupdf` now also returns the submodule's own documentation — this
   repository's plans, which discuss MuPDF constantly and are not Tinker's to
   edit — plus a committed build artefact (`apps/app/dist/assets/*.js.map`)
   and a comment in `packages/backend/src/types.ts` about "MuPDF's WASM build
   in a browser", which is Tinker's web plan rather than its engine. The
   criterion is corrected in the milestone table.

### Owner decisions recorded here — answered 16 August 2026

Recorded with their options when this plan was written, because none of the
three was decidable before the engine existed. All three now have an answer,
and each answer is written where the options were rather than in a section of
its own, so that nobody reads the question without the reply.

**1. EPUB, XPS and CBZ — built natively, inside tinker-pdf.**
*Answered 16 August 2026.* `Doc::Other` exists only because MuPDF was
multi-format. [gaps/28](gaps/28-tinker-integration-decisions.md) costed three
options — drop them, keep MuPDF for those three formats alone, or convert out
of process — and recommended dropping them. The answer is a fourth option that
document does not contain: the three formats become capabilities of this
engine, each with a gap plan of its own written after 28 closes.

It reaches the licensing outcome dropping them was recommended *for*: MuPDF
leaves Tinker's shipped tree entirely, and takes the AGPL dependency, the
vendored MSVC patch and the iOS App Store blocker with it. What it costs is
this repository's stated identity — "tinker-pdf is a PDF engine and always
will be" is no longer true, and every place that said so is amended
deliberately rather than left to rot.

Sized with those costs put to the owner explicitly. **CBZ is S**: a ZIP of
images, which the engine's own inflate and JPEG decoder already do most of.
**XPS is L**: an OPC ZIP, a hand-rolled XML parser, and fixed-page markup that
maps closely onto the path, glyph and brush calls the `Device` seam already
has. **EPUB is XL+**: XHTML with a CSS cascade, a box model, line breaking,
pagination and font fallback is a layout engine rather than a renderer, and on
its own it is larger than the whole twenty-eight-plan gap programme that
precedes this decision. Ruling 3 and CONTRIBUTING rule 1 hold throughout — the
ZIP reader, the XML parser and every line of the CSS are ours.

The capability matrix (`caps_get`) stays the mechanism either way: until a
format's plan lands, its absence is reported rather than discovered.

**2. Forms JavaScript — a hand-rolled ECMAScript subset, and it is built.**
*Answered by [gaps/27](gaps/27-form-calculations-decision.md), closed
`07dd4b0`..`7c7b52d`.* The option taken is **A**, against that document's own
recommendation of B, and the reason is that A's precondition had been built in
the meantime: option B's deciding argument is that a half-implementation is
worse than none *unless the writes are transactional*, and PRE-E built
`DocumentEditor::transaction` before gap 27 was picked up. Option B's reader —
`/AA`, `/CO`, `/Names /JavaScript` and the catalog's `/AA`, surfaced as source
text — landed first and independently green, so A sits on top of it rather
than instead of it, and removing the interpreter would leave a working reader.
Phase [11](11-forms.md)'s open item is closed with it; `boa` was never a
candidate, because ruling 3 rules out a JavaScript crate. `formCalc` becomes
true in Tinker's capability matrix at integration, with the honest caveat that
nothing recalculates by itself — *when* a calculation runs is a host's policy,
and `recalculate()` is the door.

**3. Tinker's licence — permissive, MIT OR Apache-2.0, matching the engine.**
*Answered 16 August 2026.* With MuPDF gone the only AGPL in Tinker's shipped
tree goes with it, so nothing forces the choice, and the owner has chosen the
engine's own terms rather than staying copyleft. `deny.toml` updates to match:
`AGPL-3.0` leaves the allowlist along with both `mupdf`/`mupdf-sys`
exceptions, and the workspace `license` field and `package.json` change with
them.

Two things the owner should see beside that answer. **Relicensing existing
Tinker code needs its contributors' agreement**, which is a step outside this
repository and outside this plan — the decision is recorded here, the consent
is collected there. And the iOS App Store blocker documented in
`docs/mupdf-limitations.md` was about the *dependency*, not about Tinker's own
licence: it clears when MuPDF leaves, whatever Tinker chooses for itself.

## Milestones

| # | Deliverable | Exit criteria | Size |
| --- | --- | --- | --- |
| 15.1 | Swap PR series: engine module rewritten, `legacy-mupdf` A/B feature | Full Tinker suite green on tinker-pdf; A/B differential run on a personal document set shows no regressions worth blocking | M |
| 15.2 | Golden regeneration + fixture freeze | New goldens reviewed and committed; `visual_regression.rs` green; fixtures README documents the binary exception | S |
| 15.3 | Deletion checklist executed | `rg -i mupdf` in Tinker, **excluding `engine/`, `apps/app/dist/` and `packages/*/dist/`**, returns only historical docs — the submodule is this engine's own tree and its plans discuss MuPDF throughout; CI green **with no C toolchain installed anywhere**; clean-machine build is `rustup + cargo build` | S |
| 15.4 | App smoke + release | Tauri app opens, scrolls, searches, renders fixtures and real documents; `tinker-cli info/render/text` parity; a release ships from the simplified pipeline | S |

## Dependencies

Checkpoint B ([PLAN.md](../PLAN.md)), which requires phases
[01](01-cos-and-object-model.md)–[09](09-writing.md) and the
[14](14-testing-and-corpora.md) machinery. [13-bindings](13-bindings.md) is
not a prerequisite for desktop integration but its wasm package is what
replaces mupdf.js in Tinker's web plan — sequence it before the web milestone
of Tinker's own roadmap resumes.

### Amendment, August 2026: a blocker this plan does not name

Three things this file assumes, none of which currently holds. Recorded here
rather than rewritten above, because the plan is right about the work and
wrong only about its readiness.

**The precondition is not met and cannot yet be measured.** Checkpoint B wants
corpus render parity ratcheted at 95% or better. No corpus has ever been run —
eight real files have been through `tpdf`, total. See
[gaps/23](gaps/23-corpus-runner.md). Integration cannot start against evidence
that does not exist, and the number is one corpus run away rather than far.

**Tinker must supply a `FontProvider` or every non-embedding document renders
textless.** The engine bundles no faces and reads no font directories — by
design, so it has no OS dependencies and builds identically on wasm. That
makes the host responsible, and Tinker is the host. This is a render-parity
blocker on the widest class of real files, it appears in no phase plan, and it
must land *before* any golden comparison or every comparison measures the
absence of text. See [gaps/28](gaps/28-tinker-integration-decisions.md).

**The deletion checklist is unverified against Tinker's current tree.** Its
paths were checked at planning time, which was the scaffolding commit. The
freeze exempts dependency and security bumps, so drift is guaranteed. Re-check
every path before using it as a checklist.

The three owner decisions below are still open. [gaps/27](gaps/27-form-calculations-decision.md)
recommends an answer to the forms-JavaScript one;
[gaps/28](gaps/28-tinker-integration-decisions.md) costs the other two.

## Risks

| Risk | Mitigation |
| --- | --- |
| Parity tests pass but real documents regress (the corpus missed something) | The A/B `legacy-mupdf` differential run on live documents before deletion; capability warnings make degradation visible in the UI instead of silent |
| Golden review normalizes a real quality loss | pdfcmp heat maps reviewed page-by-page once; the ratcheted corpus budget from [14](14-testing-and-corpora.md) is the objective backstop |
| Tinker rot during the freeze bites at integration (Tauri, deps, CI drift) | The freeze exempts dependency/security bumps; the integration branch rebases early and often during 15.1, not at the end |
| Scope temptation: "while we're in here" refactors of the actor/registry | The swap changes engine module internals only; anything else is a named follow-up PR after 15.4 |
