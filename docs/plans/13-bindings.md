# Phase 13 — Bindings

When this phase is done, tinker-pdf is installable in four ecosystems —
`cargo add tinker-pdf`, `npm i @tinker/pdf`, `pip install tinker-pdf`,
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
Tinker's web plan). Two deliberate calls:

- **Copy is the default, views are opt-in.** `bitmap.data()` returns a copied
  `Uint8Array`; `bitmap.view()` returns a view into wasm memory documented as
  **invalidated by any allocation that grows the memory** — the classic
  footgun gets the footgun-shaped name, and the safe call gets the short one.
- **No wasm threads in v1.** The Send+Sync superiority is native-side; wasm
  runs one engine per worker, which sidesteps SharedArrayBuffer/COOP/COEP
  hosting requirements entirely (a constraint Tinker's web plan already
  treats as a feature).

Size budget: **< 2.5 MB gzipped** for the engine wasm including base-14
substitute *metrics*; the substitute font *programs* (Liberation,
[05-fonts](05-fonts.md)) ship as a separate lazily-fetched asset so documents
with fully embedded fonts never pay for them. CI fails the build over budget.

### .NET

csbindgen generates P/Invoke declarations from `tinker-pdf-ffi`'s actual
source — no hand-maintained extern block to drift. A thin safe wrapper
(`TinkerPdf.Document : IDisposable`) owns handle lifetime, maps status codes
to exceptions, and exposes `ReadOnlySpan<byte>` over bitmap views (zero-copy,
lifetime tied to the SafeHandle). NuGet layout:
`runtimes/{win-x64,linux-x64,osx-arm64}/native/` with the cdylib per RID.

### Release

One workspace version. `cargo xtask release` from a tag runs, in order:
`cargo publish` (leaves → facade → ffi), `maturin publish`, `wasm-pack
publish`, `dotnet pack && dotnet nuget push`. Any step failing halts the
chain; re-running skips already-published versions so a partial release is
resumable, not corrupt.

## Milestones

| # | Deliverable | Exit criteria | Size |
| --- | --- | --- | --- |
| 13.1 | C ABI covering open/auth/meta/text/render + header | ffi smoke test (open, render, free; ASAN-clean) green in CI on linux/windows/macos | S |
| 13.2 | Python package | `pip install` from CI artifact; render + buffer-protocol test; GIL released during render (thread-scaling test) | S |
| 13.3 | JS/wasm package | `npm i` from CI artifact; browser demo page renders an uploaded PDF in a worker; size budget gate green | M |
| 13.4 | .NET package | `dotnet add package` from CI feed; render + span test on all three RIDs | S |
| 13.5 | One-tag release automation + leaf-crate publishing | Dry-run release from a tag produces all four artifacts + crates.io set; resumability tested by killing a step | S |

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
