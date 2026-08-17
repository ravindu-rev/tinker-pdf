# Phase 00 — Architecture

When this phase is done, the tinker-pdf workspace exists and compiles — every crate, on every target including `wasm32-unknown-unknown` — with almost nothing implemented but everything decided: crate boundaries, the hand-rolled dependency policy, the error and warning model, capability degradation, the determinism contract, the concurrency model, and the facade's public shape. It is shaped this way because these are the decisions every later phase inherits and cannot cheaply reverse. The phase is small in effort and large in consequence: an engine that panics on fuzz input, drifts by one pixel between x86 and wasm, or leaks internal types through its facade is not fixed later — it is prevented here or not at all.

## Scope

- Workspace layout and the crate DAG, dependency direction enforced in CI (a leaf crate acquiring a PDF-typed dependency is a build failure).
- The hand-rolled policy and its exact exemption boundary, enforced by `cargo-deny` with an explicit allowlist.
- Error model: never-panic on untrusted input (fuzz-enforced), per-crate error enums converging in the facade, structured warnings carried on results (`Bitmap.warnings`).
- `Capability` flags and the degrade contract — placeholder plus warning, never a failed page — and the corpus hit-rate gate that decides when a deferred capability gets built.
- Determinism policy: fixed-point coverage accumulation, no platform libm in hot paths, bit-identical output across all CI targets including wasm, goldens compared byte-for-byte between targets.
- Concurrency model: `Document(Arc<DocInner>)` is `Send + Sync`; immutable `Arc<[u8]>` source; sharded object cache; `OnceLock` file encryption key (7.6.2 derives exactly one file key per document, so compute-once is the natural shape); `authenticate(&self)` via interior mutability.
- API stability policy: everything is 0.x until Checkpoint B; the facade freezes at 0.1.0.
- Facade API skeleton with documented contracts: `Document`/`Page`/`TextPage`/`Bitmap`/`RenderOptions`/`CancelToken`, the COS escape hatch, `Quad::bounds()`.
- The outward-rounding contract for raster dimensions (A4 at 150 dpi is 1240×1755) as a documented, unit-tested API guarantee.
- CI skeleton: fmt, `clippy -D warnings`, `cargo-deny`, test matrix (Linux/Windows/macOS x86_64 + aarch64 + wasm32), cross-target golden comparison job, `cargo-fuzz` scaffolding per leaf crate.

## Non-goals

- No parsing, decoding, rendering, or writing implementation. File syntax and xref (including 7.5.8 xref streams and repair) are [01-cos](01-cos-and-object-model.md); codecs are [02-filters](02-filters.md); encryption algorithms are [03-crypto](03-encryption.md); font parsing is [05-fonts](05-fonts.md); color is [08-rendering-device](08-rendering-device.md); the interpreter and text device are [06-content-and-text](06-content-and-text.md); the rasterizer is [07-raster](07-rasterizer.md) and [08-render](08-rendering-device.md); serialization and mutation are [09-write](09-writing.md).
- No Tinker integration. Tinker feature work is frozen; the engine reaches parity standalone against `crates/tinker-core/tests/` and integrates in the final phase — see [PLAN.md](../PLAN.md) for the checkpoint definitions.
- No PDF 2.0 work. The baseline is PDF 1.7 (ISO 32000-1); 2.0 deltas are tracked separately in the register owned by [PLAN.md](../PLAN.md).
- No tool internals. `pdfcmp` (own perceptual diff), `oracle-diff` (mutool/pdftoppm/pdfium_test as CI subprocess oracles only, never linked), and `tpdf` (debug CLI) get their DAG positions here and their designs in their own plans.
- No C ABI design. `tinker-pdf-ffi` exists in the workspace so the DAG is complete; its surface is designed when there is something to bind.
- No `DocumentEditor` design beyond the sketch needed to prove open readers stay valid under mutation. Full design belongs to [09-write](09-writing.md).

## Design

### Crate DAG

```text
tinker-pdf-math ────→ tinker-pdf-color ──┐
              └─────→ tinker-pdf-raster ─┼───────────────────────────────┐
tinker-pdf-filters ─┬─→ tinker-pdf-font ─┤                               ↓
                    ├─→ tinker-pdf-zip ──┼───────────────────────────────┤
tinker-pdf-crypto ──┴─→ tinker-pdf-cos ──┴─→ tinker-pdf-content ─→ tinker-pdf-render ─→ tinker-pdf ─→ tinker-pdf-ffi

tools: pdfcmp (no PDF deps) · oracle-diff (subprocess oracles) · tpdf (depends on facade)
```

**Seven leaf crates** — `filters`, `crypto`, `font`, `color`, `raster`, `math`, `zip` — are bytes-in/values-out with zero PDF types. This is the property that makes each one independently fuzzable: a fuzz target hands `tinker-pdf-font` a byte slice and expects a value or a structured error, with no COS machinery in the corpus or the crash triage. It also means a leaf can be tested against its spec (DEFLATE against RFC 1951, CFF against Adobe TN 5176) without a PDF in sight.

*Amended, August 2026, twice, and the second time is why the amendment is dated in place rather than folded in silently.* This said "five" from the day it was written. `tinker-pdf-math` arrived with ruling 4's amendment as a **second-order leaf** — `no_std`, depended on by `color` and `raster`, depending on nothing — and not one of the three prose statements of the count moved, in this file, in [99-consistency](99-consistency.md)'s ruling 8, or in [CONTRIBUTING.md](../../CONTRIBUTING.md)'s rule 3. It had drifted for weeks before [gap 29](gaps/29-cbz.md) went looking. `tinker-pdf-zip` is the seventh, added by that gap for CBZ and reused by gap 30 for XPS, and the sweep that added it is the one that found the first drift. A count written in four places and enforced in none is a fact about the documentation rather than about the code — which is why the count is stated here with the crates enumerated, so that a reader can check it against `crates/` rather than take it.

Two of the seven have a dependency, and both are leaf-to-leaf: `font -> filters` and `zip -> filters`, each argued in `xtask`'s `ALLOWED` doc comment. A leaf here means bytes in, values out, no PDF types, independently fuzzable — not "no edges". The graph cannot cycle, because `filters` depends on nothing.

`tinker-pdf-cos` owns file syntax, xref, repair, and serializers, and depends on `filters` + `crypto` because streams cannot be read without decoding and decryption. `tinker-pdf-content` is the content-stream interpreter plus `trait Device`, and ships the text device; it depends on `cos`, `font`, `color`. `tinker-pdf-render` is the rasterizing `Device` implementation over `raster`. `tinker-pdf` is the facade and the only user-facing crate; internal crates make no stability promises, ever.

The `content → Device` seam is load-bearing, not decorative. Because the text device lives in `content` and the rasterizing device in `render`, the text-extraction path never links a rasterizer — which is exactly why Checkpoint A (text/outline/metadata/encryption parity, no rasterizer) can exist as a shippable milestone rather than a fiction. A design that let the interpreter reach into raster types, even for convenience, would collapse the two checkpoints into one giant one.

Dependency direction is enforced: a CI script diffs `cargo metadata` against the declared DAG and fails on any new edge. Convenient shortcuts between crates are how seams die.

### Hand-rolled policy

License is MIT OR Apache-2.0 and everything in the engine is hand-rolled: own inflate/deflate, JPEG, CCITT, LZW, font parsers (TrueType/CFF/Type1/CID/CMap), the path+glyph rasterizer with AA, and PDF crypto (RC4, MD5, AES-128/256-CBC, SHA-2). No third-party crates for PDF logic or primitives. These are given decisions; this document states the boundary, not the argument.

The exemption boundary is exact: a crate is exempt only if it is dev/build/binding tooling that never ships in a user's artifact and never touches PDF bytes at runtime. The exempt list is `proptest`, `criterion`, `cargo-fuzz`, `PyO3`, `wasm-bindgen`, `maturin`, `csbindgen`. Anything else — including "just a small helper" crates like a hash or a bit-reader — fails `cargo-deny`, which runs with an explicit allowlist naming those seven and nothing more. The same trick Tinker uses to police MuPDF features (`scripts/check-mupdf-deps.mjs`) applies: the policy lives in CI, not in review vigilance.

The practical consequences the rest of the plan leans on: no C toolchain anywhere, so `wasm32-unknown-unknown` is a plain `cargo build` from day one; every byte of the decode path is ours, so the never-panic and determinism policies are enforceable rather than aspirational; and no upstream's leniency choices leak into ours.

### Error model

Three rules, in priority order.

**Never panic on untrusted input.** Every parser and decoder returns `Result`; indexing is checked; arithmetic on file-derived values is checked or saturating; recursion has explicit depth limits and decoders have dimension/allocation budgets (ISO 32000-1 Annex C is the starting point for limits, but ours are hardening limits, not conformance limits). This is fuzz-enforced from this phase: each leaf crate plus `cos` and `content` gets a `cargo-fuzz` target in this phase's scaffolding, fuzz builds run with overflow checks on, and any panic — including a slice index or an allocator abort from an unbudgeted allocation — is a bug with a reduced fixture committed to the corpus.

**Errors are per-crate enums converging in the facade.** `FilterError`, `CryptoError`, `FontError`, `ColorError`, `RasterError`, `CosError`, `ContentError` — each crate speaks its own vocabulary with no upward dependencies. The facade defines one public `Error` with `#[from]` conversions and a stable `ErrorKind` for callers that branch (Tinker branches on stable codes like `PASSWORD_REQUIRED`; the facade must support that idiom natively).

**A page that produces a value is a success, and its problems ride on the value.** Real PDFs are broken in ways users cannot fix, so leniency policy is repair-over-reject: hard errors are reserved for "no value can be produced" (not a PDF, wrong password, cancelled, page index out of range). Everything else — a bad xref entry that got repaired, an unparseable annotation, a degraded image — becomes a structured warning carried on the result:

```rust
pub struct Bitmap {
    // pixels, dimensions, format ...
    pub warnings: Vec<Warning>,
}

pub enum Warning {
    Repaired { what: RepairKind },
    Degraded { capability: Capability, object: ObjRef },
    Skipped { what: SkipKind, object: ObjRef },
}
```

`TextPage` carries the same field. Warnings are data, not log lines: `pdfcmp` and the parity suite assert on them, and Tinker can surface them in UI. Nothing is silently dropped and nothing non-fatal aborts a page.

### Capability flags

Some decoders are deliberately deferred — JBIG2, JPEG 2000, arithmetic-coded JPEG are the known initial set — and the architecture must make deferral safe rather than pretending completeness:

```rust
#[non_exhaustive]
pub enum Capability {
    Jbig2,
    Jpx,
    ArithJpeg,
    // grows as deferrals are identified; never shrinks within 0.x
}

pub fn capabilities() -> &'static [Capability]; // what this build implements
```

The degrade contract: hitting an unimplemented capability during rendering draws a placeholder (flat mid-gray in the image's rect) and pushes `Warning::Degraded { capability, object }`. It never fails the page and never panics. This is the same posture throughout: a missing codec is a quality problem, not a correctness problem.

Deferred capabilities get built on evidence, not vibes: `oracle-diff` corpus runs record per-capability hit rates (what fraction of corpus pages degrade, and by how much perceptual difference in `pdfcmp`), and those dashboards are the gate. A capability whose hit rate stays negligible stays deferred; one that moves the parity needle gets scheduled. This keeps codec effort pointed at real documents instead of spec completionism.

### Determinism

The contract: rendering the same bytes with the same options produces bit-identical output on every supported target — x86_64 and aarch64, all three desktop OSes, and wasm32. Not "visually identical", bit-identical. This is what makes single-golden CI possible: `pdfcmp` goldens are stored once, and a cross-target CI job renders the same fixtures on every target and compares hashes. A perceptual diff that also has to absorb platform noise can hide real regressions inside its tolerance; ours does not have to.

How it is achieved:

- **Fixed-point coverage accumulation** in the rasterizer. Edge walking and coverage sums use integer fixed-point, so there is no float summation-order or FMA-contraction variance to leak into pixel values. (The detailed rasterizer design is [07-raster](07-rasterizer.md); the *policy* that it must be fixed-point is set here, because it is an architecture constraint, not an implementation detail.)
- **No platform libm in hot paths.** Transcendentals (needed in color transforms and shading functions) go through our own implementations with committed test vectors. Platform `sin`/`pow` differ in last-ulp results across libms, and last-ulp differences become visible pixels after quantization.
- **No environment-dependent behavior**: no locale-sensitive parsing, no `HashMap` iteration order in any output path (ordered structures or explicit sorts where order reaches output), no time or randomness in the render path.

The determinism CI job exists from this phase, initially comparing trivial artifacts (fixed-point math kernels, transcendental test vectors) so the harness is proven before there are pixels to protect. Any cross-target divergence is a build-stopping failure, never a tolerance bump.

### Concurrency

```rust
pub struct Document(Arc<DocInner>);
// Send + Sync + Clone (cheap)

struct DocInner {
    source: Arc<[u8]>,                                    // immutable after open
    xref: Xref,                                           // immutable after open+repair
    cache: [RwLock<HashMap<ObjNum, Arc<CosValue>>>; 16],  // sharded object cache
    file_key: OnceLock<FileKey>,                          // 7.6.2: one key per document
    auth: AuthState,                                      // interior mutability
}
```

The source bytes are an immutable `Arc<[u8]>`; nothing ever writes to them. Parsed objects are immutable `Arc<CosValue>`s in a sharded cache — a fixed array of `RwLock<HashMap>` shards keyed by object number. On a cache miss two threads may race to parse the same object; both produce identical values because parsing is deterministic over immutable bytes, so last-write-wins is correct and the only cost is a wasted parse. No per-entry locks until profiling says otherwise.

`authenticate(&self)` takes a shared reference, storing the outcome (including `AuthLevel::User` vs `AuthLevel::Owner`, which the spec distinguishes and callers need) through interior mutability. This deliberately fixes two frictions Tinker recorded against MuPDF (`docs/mupdf-limitations.md` items 3 and 8 in the Tinker repo): authentication level was unrecoverable through the safe API, and `authenticate(&mut self)` forced awkward call ordering.

The consequence worth stating plainly: **concurrent reads and renders of one `Document` from many threads are simply safe.** Tinker's actor-per-document model exists because MuPDF contexts are not thread-safe; this design dissolves that constraint at the source. Whether Tinker later simplifies its actor model is Tinker's call at integration time — the engine's job is to stop forcing the issue.

Mutation comes later ([09-write](09-writing.md)) as an exclusive copy-on-write `DocumentEditor`: the editor builds a new revision out of new and borrowed `Arc`s while every open reader keeps its consistent snapshot alive through its own `Arc`s. Readers never observe a half-applied edit and never dangle. This phase only proves the shape compiles and the reader guarantee holds; the editor API itself is out of scope here.

On wasm32 the same types compile single-threaded; the library spawns no threads and owns no global mutable state beyond `OnceLock`-style caches, so threading is entirely the embedder's business on every target.

### API stability and the facade

Everything is 0.x until Checkpoint B. Internal crates never gain stability promises at all. When Checkpoint B (full render+write parity — the integration bar) is reached, the facade freezes at 0.1.0 and breaking changes become deliberate events. Freezing earlier would lock in guesses; the parity suite — Tinker's own tests in `crates/tinker-core/tests/` — exercises the facade as a real consumer before the freeze, which is the cheapest API review available.

The facade skeleton, compiled with `todo!()` bodies in this phase so signatures and doc-comment contracts are reviewable code rather than prose:

```rust
pub struct Document(Arc<DocInner>);

impl Document {
    pub fn open(bytes: impl Into<Arc<[u8]>>) -> Result<Document, Error>;
    pub fn needs_password(&self) -> bool;
    pub fn authenticate(&self, password: &str) -> Result<AuthLevel, Error>; // &self — see Concurrency
    pub fn page_count(&self) -> usize;
    pub fn page(&self, index: usize) -> Result<Page, Error>;
    pub fn outline(&self) -> Vec<OutlineItem>;
    pub fn metadata(&self) -> Metadata;
    pub fn cos(&self) -> CosView<'_>; // escape hatch: typed, read-only COS access
}

impl Page {
    pub fn size(&self) -> Size; // points, CropBox-derived
    pub fn render(&self, opts: &RenderOptions) -> Result<Bitmap, Error>;
    pub fn text(&self) -> Result<TextPage, Error>; // links no rasterizer
}

pub struct RenderOptions {
    pub scale: f32,
    pub format: PixelFormat,
    pub cancel: Option<CancelToken>,
    // non_exhaustive; constructed via Default + builder methods
}

pub struct CancelToken(Arc<AtomicBool>); // cooperative; checked at band boundaries

impl TextPage {
    pub fn search(&self, needle: &str) -> Vec<Quad>;
}

impl Quad {
    pub fn bounds(&self) -> Rect; // the method MuPDF made everyone write themselves
}
```

The `cos()` escape hatch is deliberate: Tinker's page-surgery plans repeatedly needed raw object access MuPDF hid, and the answer here is a supported read-only view rather than an `unsafe` shim culture. `Quad::bounds()` exists because Tinker's limitations table (item 8) proves its absence is a paper cut every caller hits; its docs carry the same caveat — the collapse is exact only for upright text.

### Outward rounding

Raster dimensions are `ceil` per axis of the scaled page box: A4 (595.276 × 841.89 pt) at 150 dpi renders to exactly **1240 × 1755**, never 1240 × 1754. A page must not lose its last row of pixels to rounding. Tinker discovered this as undocumented MuPDF behavior and depends on it; here it is promoted to a documented API guarantee on `RenderOptions`/`Bitmap` with a unit test pinning that exact A4 case, so it is a contract rather than a rediscovered surprise — and so parity comparisons against Tinker's existing baselines line up pixel-for-pixel in dimensions from the first render.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Workspace skeleton: all ten crates plus `pdfcmp`/`oracle-diff`/`tpdf` compile (empty) on stable Rust; DAG-enforcement script | `cargo build --workspace` green on all desktop targets and `--target wasm32-unknown-unknown`; CI fails a deliberately added illegal crate edge | S |
| 2 | Shared foundations: geometry types (`Point`/`Rect`/`Matrix`/`Quad::bounds()`), per-crate error enums, `Warning`, `Capability`, `CancelToken`, outward-rounding helper | Unit tests pass, including A4@150dpi = 1240×1755; facade re-exports compile; `cargo doc` clean with contracts stated on every public item | S |
| 3 | CI skeleton: fmt, `clippy -D warnings`, `cargo-deny` with the seven-crate exemption allowlist, test matrix (Linux/Windows/macOS/wasm-under-node), determinism job, `cargo-fuzz` scaffolding for the leaves + cos + content | `cargo-deny` fails when a non-exempt runtime dependency is added in a test PR; `cargo fuzz build` green for every target; determinism job compares fixed-point/transcendental vector artifacts byte-identically across all matrix targets | S |
| 4 | Facade skeleton: `Document`/`Page`/`TextPage`/`Bitmap`/`RenderOptions`/`CancelToken`/`CosView` signatures with `todo!()` bodies and doc-comment contracts (rounding, warnings, degrade, `&self` auth) | Compiles and documents cleanly; API shape reviewed and recorded against [PLAN.md](../PLAN.md); parity-suite skeleton can import the types | S |

Milestone rows overlap heavily and share scaffolding; the phase as a whole sits in the S band.

### Amendment, August 2026: `cos -> font`

The DAG above lists `cos -> {filters, crypto}`. The shipped graph has a third
edge, `cos -> font`, and it is deliberate.

Reading a font *dictionary* — its `/Encoding` and `/Differences`, its
`/ToUnicode` CMap, its standard-14 metrics — is COS work: it is what turns a
`/Font` resource into widths and Unicode, and it belongs beside the rest of the
object model. Doing it needs the leaf font crate's CMap parser and encoding
tables. The alternatives were a fourth crate between them whose only job is to
hold two tables, or duplicating the tables, and both are worse than an edge
that still points from a higher layer down to a leaf.

It does not weaken ruling 8: `tinker-pdf-font` remains PDF-free, takes bytes
and returns values, and is independently fuzzable.

The graph is enforced by `cargo xtask dag`, which runs in CI. It exists
because nothing else can catch this: an undeclared edge compiles.

## Dependencies

Phase 00 depends on nothing — it is the root. It unblocks everything, and unblocks the leaves *in parallel*: [02-filters](02-filters.md), [03-crypto](03-encryption.md), [05-fonts](05-fonts.md), [08-rendering-device](08-rendering-device.md), and [07-raster](07-rasterizer.md) have no dependencies on each other and can start the moment their crate skeletons and fuzz targets exist. [01-cos](01-cos-and-object-model.md) starts against stub filter/crypto traits immediately and binds to real implementations as they land. [06-content-and-text](06-content-and-text.md) needs cos/font/color surfaces; [08-render](08-rendering-device.md) needs content and raster; [09-write](09-writing.md) needs cos and the `DocumentEditor` reader guarantee proven here. Checkpoint A and B definitions and the integration sequencing live in [PLAN.md](../PLAN.md).

## Risks

| Risk | Mitigation |
| --- | --- |
| Cross-target bit-identical determinism proves harder than planned (float contraction, wasm rounding modes) | Policy is structural, not corrective: fixed-point coverage and own transcendentals remove the variance sources instead of chasing them; determinism CI runs from milestone 3 on trivial kernels, so divergence surfaces before any rasterizer code exists |
| Hand-rolled scope hides underestimated monsters (progressive JPEG, CFF hinting edge cases) | The capability degrade path makes any codec deferrable without blocking the pipeline; corpus hit-rate dashboards keep the build-order honest; leaf isolation means a hard codec never stalls unrelated phases |
| Facade shape is wrong and freezes wrong at 0.1.0 | Freeze only at Checkpoint B, after Tinker's real test suite has consumed the API; internal crates stay unstable forever, so only the thin facade carries freeze risk |
| Never-panic collides with adversarial resource exhaustion (allocation bombs, deep recursion) | Explicit budgets — dimension caps, recursion depth, allocation limits — returning structured errors; fuzz builds run with budgets on so exhaustion paths are exercised, not just panics |
| Sharded cache races cause duplicate parse work under contention | Accepted by design: objects are immutable so duplication is waste, not incorrectness; measure before adding complexity |
| Policy drift: a convenient runtime crate slips in during a busy phase | `cargo-deny` allowlist is CI-enforced and the exemption list is named in this document; adding to it requires editing this plan, which makes the exception a reviewed decision |
| DAG erosion via convenience dependencies (content reaching into raster) | `cargo metadata` diff against the declared DAG in CI; the Checkpoint A build (`text()` without a rasterizer linked) is itself a standing test of the seam |

---

Sibling plans: [01-cos](01-cos-and-object-model.md) · [02-filters](02-filters.md) · [03-crypto](03-encryption.md) · [05-fonts](05-fonts.md) · [08-rendering-device](08-rendering-device.md) · [06-content-and-text](06-content-and-text.md) · [07-raster](07-rasterizer.md) · [08-render](08-rendering-device.md) · [09-write](09-writing.md). Master plan and checkpoint definitions: [PLAN.md](../PLAN.md).
