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

Exact paths, verified against Tinker's tree at planning time:

- `third_party/mupdf-msvc/` (the vendored patched wrapper, both patches).
- `scripts/vendor-mupdf-patch.mjs`, `scripts/check-mupdf-deps.mjs`, the
  `check:mupdf` entry in `package.json`, and the "MuPDF dependency
  discipline" CI step.
- `Cargo.toml`: the `[workspace.dependencies] mupdf` block,
  `[patch.crates-io] mupdf`, `[profile.dev.package.mupdf-sys]`.
- `.cargo/config.toml`: `MUPDF_MSVC_PLATFORM_TOOLSET`.
- `deny.toml`: `AGPL-3.0` in the allowlist, both `mupdf`/`mupdf-sys`
  exceptions.
- CI: clang install steps in `ci.yml`; in `release.yml` the 90-minute
  timeouts, per-arch native macOS runner justification, and the
  source-archive check for `third_party/mupdf-msvc/Cargo.toml`.
- `CONTRIBUTING.md`: the C-toolchain prerequisites and both Windows build
  quirks; `.gitattributes` comments citing the vendored wrapper.
- `docs/mupdf-limitations.md`: gains a historical banner ("resolved by
  tinker-pdf; kept as the record of why"); `docs/upstream/` bug drafts
  close with pointers.
- The mobile CI jobs lose their "MuPDF cross-compilation is the riskiest
  item" rationale — pure Rust cross-compiles with rustup targets; the jobs
  stay, their risk register shrinks.

### Owner decisions recorded here, decided at integration

1. **EPUB/XPS/CBZ.** `Doc::Other` exists only because MuPDF was
   multi-format. Options: drop (viewer becomes PDF-only, honest), convert
   via external tool at open, or a later dedicated module. The capability
   matrix (`caps_get`) makes any choice UI-clean.
2. **Forms JS.** Arrives with engine phase [11](11-forms.md)'s open item
   (own ES-subset interpreter vs `boa`); until then `formCalc: false` in
   capabilities, exactly as Tinker's web plan already gates it.
3. **Tinker's license.** With MuPDF gone, the only AGPL in the shipped tree
   goes with it. Stay AGPL (copyleft app over permissive engine — coherent)
   or relicense (maximally adoptable; the iOS App Store blocker was
   specifically *Artifex's* copyright and dies either way). Nothing in this
   plan forecloses either; `deny.toml` updates to match the choice.

## Milestones

| # | Deliverable | Exit criteria | Size |
| --- | --- | --- | --- |
| 15.1 | Swap PR series: engine module rewritten, `legacy-mupdf` A/B feature | Full Tinker suite green on tinker-pdf; A/B differential run on a personal document set shows no regressions worth blocking | M |
| 15.2 | Golden regeneration + fixture freeze | New goldens reviewed and committed; `visual_regression.rs` green; fixtures README documents the binary exception | S |
| 15.3 | Deletion checklist executed | `rg -i mupdf` in Tinker returns only historical docs; CI green **with no C toolchain installed anywhere**; clean-machine build is `rustup + cargo build` | S |
| 15.4 | App smoke + release | Tauri app opens, scrolls, searches, renders fixtures and real documents; `tinker-cli info/render/text` parity; a release ships from the simplified pipeline | S |

## Dependencies

Checkpoint B ([PLAN.md](../PLAN.md)), which requires phases
[01](01-cos-and-object-model.md)–[09](09-writing.md) and the
[14](14-testing-and-corpora.md) machinery. [13-bindings](13-bindings.md) is
not a prerequisite for desktop integration but its wasm package is what
replaces mupdf.js in Tinker's web plan — sequence it before the web milestone
of Tinker's own roadmap resumes.

## Risks

| Risk | Mitigation |
| --- | --- |
| Parity tests pass but real documents regress (the corpus missed something) | The A/B `legacy-mupdf` differential run on live documents before deletion; capability warnings make degradation visible in the UI instead of silent |
| Golden review normalizes a real quality loss | pdfcmp heat maps reviewed page-by-page once; the ratcheted corpus budget from [14](14-testing-and-corpora.md) is the objective backstop |
| Tinker rot during the freeze bites at integration (Tauri, deps, CI drift) | The freeze exempts dependency/security bumps; the integration branch rebases early and often during 15.1, not at the end |
| Scope temptation: "while we're in here" refactors of the actor/registry | The swap changes engine module internals only; anything else is a named follow-up PR after 15.4 |
