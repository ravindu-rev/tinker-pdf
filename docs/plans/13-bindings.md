# Phase 13 — Bindings

When this phase is done, tinker-pdf is installable in four ecosystems —
`cargo add tinker-pdf`, `npm i tinker-pdf-js`, `pip install tinker-pdf`,
`dotnet add package TinkerPdf` — and all four are projections of one facade
crate. The shape is the whole design: **no binding contains logic**. Every
language layer is a mechanical translation of `tinker-pdf`'s public API
(ruling 11 in [99-consistency](99-consistency.md)); if a binding needs
behavior, the facade grows it first and every language gets it at once. This
is what MuPDF's ecosystem never had — its Rust, Java, JS and Python wrappers
each reimplemented different subsets with different bugs, and Tinker's
limitations document is substantially a catalog of one wrapper's gaps.

## Scope

- **C ABI** (`tinker-pdf-ffi`): the native-interop foundation.
- **Python**: PyO3 directly over the facade, wheels via maturin.
- **JavaScript/wasm**: wasm-bindgen directly over the facade, npm via
  wasm-pack.
- **.NET**: csbindgen-generated P/Invoke over the C ABI, safe C# wrapper,
  NuGet.
- **Release automation**: one tag publishes everything, driven by `xtask`.
- Leaf crates (`tinker-pdf-filters`, `-crypto`, `-font`, `-raster`, `-color`)
  also published to crates.io — an own inflate, JPEG decoder and rasterizer
  are independently useful, and publishing forces the interface discipline
  ruling 8 already demands.

## Non-goals

- Node native addon (napi-rs). Additive later lane; Node runs the wasm
  package meanwhile.
- uniffi. Evaluated and rejected for .NET: it imposes a foreign type model
  and a runtime where csbindgen just reads `tinker-pdf-ffi`'s real
  signatures; revisit only if Kotlin/Swift bindings become wanted, where
  uniffi's multi-language story pays.
- Async APIs. The engine is CPU-bound and synchronous; host languages wrap
  it in their own executors (`py.allow_threads`, workers, `Task.Run`) better
  than the engine can guess.
- Stable C ABI guarantees before the facade freezes at 0.1.0
  ([00-architecture](00-architecture.md)); until then the header regenerates
  every release.

## Design

### C ABI (`tinker-pdf-ffi`)

```c
/* cbindgen-generated tinker_pdf.h — sketch */
typedef struct tpdf_document tpdf_document;   /* opaque; boxed Document clone (Arc) */
typedef struct tpdf_bitmap   tpdf_bitmap;

tpdf_status tpdf_document_open_bytes(const uint8_t *bytes, size_t len,
                                     const tpdf_open_options *opts,
                                     tpdf_document **out);
tpdf_status tpdf_document_authenticate(tpdf_document *doc, const char *password,
                                       tpdf_auth_level *out);
tpdf_status tpdf_page_render(tpdf_document *doc, uint32_t page,
                             const tpdf_render_options *opts, tpdf_bitmap **out);
const uint8_t *tpdf_bitmap_data(const tpdf_bitmap *bmp, size_t *len);   /* view */
void tpdf_bitmap_free(tpdf_bitmap *bmp);
void tpdf_document_free(tpdf_document *doc);
const char *tpdf_last_error_message(void);    /* per-thread, valid until next call */
```

- **Handle-based, thread-safe by construction**: a `tpdf_document*` boxes a
  `Document` clone (an `Arc`), so handles are cheap, independently freeable,
  and callable from any thread — the core is `Send + Sync`
  ([00-architecture](00-architecture.md)) and the ABI simply inherits it.
- **Errors**: every fallible call returns a `tpdf_status` code; detail via
  `tpdf_last_error_message()` in thread-local storage. No errno games, no
  callbacks.
- **Ownership contract, stated once and enforced everywhere**: the engine
  allocates, the matching `tpdf_*_free` releases; `*_data` functions return
  borrowed views valid until the owning handle is freed. Nothing crosses the
  ABI as a caller-freed buffer.

### Python

PyO3 classes wrap facade types **directly** — not through the C ABI; the C
layer would only add a second error translation for nothing.
`Document.render()` and `.text()` release the GIL (`py.allow_threads`) — safe
because the core is `Send`, and it makes multi-page render pools work from
plain Python threads. `Bitmap` implements the buffer protocol, so
`memoryview`, `numpy.frombuffer` and Pillow ingest pixels zero-copy.
Packaging: abi3 wheels via maturin — one wheel per platform covering all
CPython versions ≥ the abi3 floor; manylinux/musllinux/windows/macos matrix
in the release CI.

### JavaScript / wasm

wasm-bindgen directly over the facade; wasm-pack builds the npm package (ESM,
worker-friendly — the expected consumer is a Web Worker per document, exactly
Tinker's web plan). Three deliberate calls:

- **ESM only, one build, wasm-pack's `web` target, and no CommonJS.** Gap 26
  required this decision be recorded; it is, at length, in
  `bindings/js/README.md`. The short of it: a dual package is not two wrappers
  over one artefact but two artefacts, because the `nodejs` target loads the
  `.wasm` with a synchronous `readFileSync` at module scope and the `web`
  target fetches it. Shipping both ships two builds that can diverge, and
  ruling 11 says a binding has no behaviour of its own to diverge with. Node
  runs the same file by handing `init` the bytes. `web` over `bundler` because
  the demo below must load from a plain `<script type="module">` with no build
  step.
- **The package is named after the crate.** This plan first sketched
  `@tinker/pdf`; wasm-pack derives the npm name *and version* from
  `Cargo.toml`, so taking the derived name means the npm version cannot drift
  from the workspace and `cargo run -p xtask -- versions` has one manifest
  fewer to police. A hand-edited `package.json` would be that manifest, and the
  file is regenerated on every build.
- **Copy is the default, views are opt-in.** `bitmap.data()` returns a copied
  `Uint8Array`; `bitmap.viewUnsafeUntilNextAllocation()` returns a view into
  wasm memory documented as **invalidated by any allocation that grows the
  memory** — the classic footgun gets the footgun-shaped name, and the safe
  call gets the short one. Gap 26 measured what "invalidated" means, because
  the sharper statement is the useful one: growing the heap *detaches* the
  ArrayBuffer, so the view does not dangle, it becomes **zero length**,
  silently, and only when an allocation crosses a page boundary.
  `bindings/js/tests/node_smoke.mjs` takes a view, forces the growth, and
  asserts the length is 0 — so this paragraph is describing observed
  behaviour rather than a warning written from memory.
- **No wasm threads in v1.** The Send+Sync superiority is native-side; wasm
  runs one engine per worker, which sidesteps SharedArrayBuffer/COOP/COEP
  hosting requirements entirely (a constraint Tinker's web plan already
  treats as a feature).

Size budget: **< 2.5 MB gzipped** for the engine wasm including base-14
substitute *metrics*. Measured August 2026, with all 202 predefined CMaps on:
2.03 MB of wasm, **1.40 MB gzipped**. The budget is now a gate in
`.github/workflows/release.yml` rather than a number in this file. The
substitute font *programs* (Liberation, [05-fonts](05-fonts.md)) ship as a
separate lazily-fetched asset so documents with fully embedded fonts never pay
for them. CI fails the build over budget.

### .NET

csbindgen generates P/Invoke declarations from `tinker-pdf-ffi`'s actual
source — no hand-maintained extern block to drift. A thin safe wrapper
(`TinkerPdf.Document : IDisposable`) owns handle lifetime, maps status codes
to exceptions, and exposes `ReadOnlySpan<byte>` over bitmap views (zero-copy,
lifetime tied to the SafeHandle). NuGet layout:
`runtimes/{win-x64,linux-x64,osx-arm64}/native/` with the cdylib per RID.

### Release

One workspace version. `cargo run -p xtask -- release` from a tag runs, in
order: `cargo publish` (leaves → facade → ffi), `maturin publish`, `wasm-pack`
then `npm publish`, `dotnet pack && dotnet nuget push`. Any step failing halts
the chain; re-running skips already-published versions so a partial release is
resumable, not corrupt.

Three amendments from gap 26, which built it:

- **`cargo xtask` is not a command here** — this repository defines no cargo
  alias. It is `cargo run -p xtask --`, everywhere.
- **The dry run is the default and `--execute` publishes**, rather than the
  other way round. A half-published release cannot be retracted from
  crates.io, so the command typed without arguments must be the harmless one.
- **The order is computed from the manifests, not written down.** A
  hand-maintained list goes stale the first time somebody adds an edge, and it
  fails halfway through an irreversible publish with an error naming the crate
  *after* the mistake. A topological sort produces it and a separate check
  validates it before anything uploads.

## Milestones

| # | Deliverable | Exit criteria | Size |
| --- | --- | --- | --- |
| 13.1 | C ABI covering open/auth/meta/text/render + header | ffi smoke test (open, render, free; ASAN-clean) green in CI on linux/windows/macos | S |
| 13.2 | Python package | `pip install` from CI artifact; render + buffer-protocol test; GIL released during render (thread-scaling test) | S |
| 13.3 | JS/wasm package | `npm i` from CI artifact; browser demo page renders an uploaded PDF in a worker; size budget gate green | M |
| 13.4 | .NET package | `dotnet add package` from CI feed; render + span test on all three RIDs | S |
| 13.5 | One-tag release automation + leaf-crate publishing | Dry-run release from a tag produces all four artifacts + crates.io set; resumability tested by killing a step | S |

**State, August 2026 (gap 26).** 13.3's browser demo exists and renders an
uploaded PDF, observed in headless Chromium; the `Uint8Array` contract below
is documented *and measured*; the size gate is green. The whole of 13.5 has
been exercised as a **dry run end to end** — 20 steps, 11 run, 1 skipped, 8
unprovable without publishing — and **nothing has been published to any
registry**. The matrix legs of 13.2 and 13.4 (linux, macOS) and the one-tag
claim itself are written in `.github/workflows/release.yml` and have never
run: no tag has triggered them. Resumability is read out of cargo's
duplicate-upload error and that string has never been seen here, so "tested by
killing a step" is not yet true.

## Dependencies

Needs the facade frozen at 0.1.0 (Checkpoint B,
[00-architecture](00-architecture.md)) — bindings against a moving API are
rework by definition; only 13.1's skeleton may start earlier to keep the ffi
crate honest. Unblocks [15-tinker-integration](15-tinker-integration.md)'s
web story and every external adopter.

## Risks

| Risk | Mitigation |
| --- | --- |
| Binding drift — a language layer grows its own behavior | Ruling 11; reviews reject logic in bindings; conformance smoke tests run the same fixture assertions in all four languages |
| wasm size creep past 2.5 MB gzipped | CI budget gate from day one; font programs already externalized; `twiggy`-style size diff on every PR that touches the facade |
| abi3/maturin or csbindgen toolchain churn | Both are exempt tooling, not shipped code — pin versions, upgrade deliberately in their own PRs |
| View-lifetime misuse across the ABI (freed bitmap, grown wasm memory) | Copy-by-default APIs everywhere; views carry the warning in their names and docs; ASAN/Miri on ffi tests |
