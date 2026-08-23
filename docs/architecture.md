# Architecture

tinker-pdf is a from-scratch, pure-Rust document engine: a workspace of
fifteen crates in which every byte of logic is this repository's own — the
inflate, the image codecs, the crypto, the font parsers, the rasterizer, the
XML parser, the CSS engine, the layout engine, and the transcendental math
they all share. `#![forbid(unsafe_code)]` holds in every engine crate, and
`cargo-deny` fails the build if a runtime dependency appears.

## Locked decisions

These are given; this document states each boundary, not the argument.

- **License:** MIT OR Apache-2.0, dual.
- **Everything hand-rolled.** All document logic and all primitives are
  ours. The only exemption is dev/build/binding tooling that never ships in
  a user's artifact and never touches document bytes at runtime: `proptest`,
  `criterion`, `cargo-fuzz`, `PyO3`, `wasm-bindgen`, `maturin`, `csbindgen`.
  Anything else — including "just a small helper" like a hash or a
  bit-reader — fails `cargo-deny`, which runs with an explicit allowlist
  naming those seven and nothing more. The policy lives in CI, not in
  review vigilance.
- **`wasm32-unknown-unknown` is a first-class target** from the first
  commit. No C toolchain exists anywhere, so the wasm build is a plain
  `cargo build`. The facade takes bytes, never paths, for the same reason.
- **Spec baseline:** PDF 1.7 (ISO 32000-1). PDF 2.0 deltas that matter
  early are tracked in [pdf20-deltas.md](pdf20-deltas.md); full 2.0
  conformance is not a current goal.
- **Formats: PDF, and CBZ, XPS and EPUB** — the three container formats
  open as a `Document` by synthesizing a real PDF at `open`, so every
  downstream capability applies to all four ([cbz](features/cbz.md),
  [xps](features/xps.md), [epub](features/epub.md)).
- **API stability:** everything is 0.x; the facade freezes at 0.1.0.
  Internal crates never gain stability promises at all.

## Crate DAG

```text
tinker-pdf-math ────→ tinker-pdf-color ──┐
              └─────→ tinker-pdf-raster ─┼───────────────────────────────┐
tinker-pdf-filters ─┬─→ tinker-pdf-font ─┤                               ↓
                    ├─→ tinker-pdf-zip ──┼───────────────────────────────┤
tinker-pdf-crypto ──┴─→ tinker-pdf-cos ──┴─→ tinker-pdf-content ─→ tinker-pdf-render ─→ tinker-pdf ─→ tinker-pdf-ffi
tinker-pdf-xml ───────────────────────────────────────────────────────────────────────→ tinker-pdf
tinker-pdf-css ─────→ tinker-pdf-layout ──────────────────────────────────────────────→ tinker-pdf

tools: pdfcmp (no engine deps) · tpdf (depends on facade)
```

**Ten leaf crates** — `filters`, `crypto`, `font`, `color`, `raster`,
`math`, `zip`, `xml`, `css`, `layout` — are bytes-in/values-out with zero
PDF types (ruling 8 defines a leaf; the definition binds, not the list).
This is the property that makes each one independently fuzzable: a fuzz
target hands `tinker-pdf-font` a byte slice and expects a value or a
structured error, with no COS machinery in the corpus or the crash triage.
It also means a leaf is tested against its own spec (DEFLATE against
RFC 1951, CFF against Adobe TN 5176) without a PDF in sight.

Four leaf-to-leaf edges exist, each pointing from a higher layer down:
`font → filters` (the CMap asset pipeline), `zip → filters` (raw DEFLATE
and CRC-32), `layout → css` (computed styles in, boxes out), and
`cos → font` (reading a font *dictionary* — `/Encoding`, `/ToUnicode`,
standard-14 metrics — is object-model work that needs the leaf's CMap
parser and encoding tables; a fourth crate whose only job is to hold two
tables would be worse). The graph cannot cycle, because `filters` depends
on nothing.

`tinker-pdf-cos` owns file syntax, xref, repair, the writer, and the strict
validator that reads a file back with the repairs turned off (ruling 13,
[verification](verification.md)).
`tinker-pdf-content` is the content-stream interpreter plus `trait Device`,
and ships the text device; the rasterizing device lives in
`tinker-pdf-render`. Because the two devices are in different crates, the
text-extraction path never links a rasterizer — the seam is load-bearing,
not decorative (ruling 7). `tinker-pdf` is the facade and the only
user-facing crate (ruling 11).

Dependency direction is enforced: `cargo xtask dag` diffs `cargo metadata`
against the declared graph in CI and fails on any new edge. It exists
because nothing else can catch this — an undeclared edge compiles.
Convenient shortcuts between crates are how seams die.

## Per-crate map

Source lines are `src/` including inline test modules, as of August 2026.

| Crate | Role | ~LOC | Feature doc | Fuzz targets |
| --- | --- | ---: | --- | --- |
| `tinker-pdf` | facade; the only public surface | 24 300 | all of [features/](README.md) | `render_page` |
| `tinker-pdf-cos` | file syntax, object store, writer, strict validator | 32 900 | [opening](features/opening.md), [document-model](features/document-model.md), [writing](features/writing.md), [forms](features/forms.md), [creation](features/creation.md) | `cos_document`, `cos_object`, `form_script` |
| `tinker-pdf-filters` | stream filters + image codecs | 21 800 | [filters](features/filters.md) | `ascii_filters`, `ccitt`, `inflate`, `jbig2`, `jpeg`, `jpx`, `lzw`, `png` |
| `tinker-pdf-crypto` | ciphers, hashes, security handlers | 3 000 | [encryption](features/encryption.md) | `crypt`, `crypt_ciphers` |
| `tinker-pdf-font` | font and CMap parsing, subsetting | 8 400 | [fonts](features/fonts.md) | `cff`, `cmap`, `sfnt`, `truetype`, `type1` |
| `tinker-pdf-content` | interpreter + `Device` seam, text device | 4 500 | [content-and-text](features/content-and-text.md) | `content_tokenizer` |
| `tinker-pdf-raster` | deterministic AA rasterizer | 5 900 | [rasterizer](features/rasterizer.md) | — (driven via `render_page`) |
| `tinker-pdf-render` | the rasterizing `Device` | 6 000 | [rendering](features/rendering.md) | — (driven via `render_page`) |
| `tinker-pdf-color` | colour spaces and functions | 1 100 | [rendering](features/rendering.md) | — |
| `tinker-pdf-math` | pinned transcendentals, `no_std` | 900 | [determinism](features/determinism.md) | — |
| `tinker-pdf-zip` | ZIP reader | 3 000 | [cbz](features/cbz.md) | `zip_archive` |
| `tinker-pdf-xml` | XML pull parser | 4 100 | [xps](features/xps.md) | `xml` |
| `tinker-pdf-css` | CSS engine | 10 800 | [epub](features/epub.md) | `css` |
| `tinker-pdf-layout` | box model, fragmentation, line breaking | 13 700 | [epub](features/epub.md) | `layout` |
| `tinker-pdf-ffi` | C ABI | 900 | [bindings](features/bindings.md) | — |

Tools: `tpdf` (debug CLI over the facade) and `pdfcmp` (perceptual
comparator), both described in [verification.md](verification.md). There is no
third: `oracle-diff`, the external-renderer harness of retired ruling 9, was
deleted with the last oracle it could have driven. `xtask` holds the workspace police
(`dag`, `libm`, `oracles`, `vendor`, `versions`, `check`) and the release,
corpus and packaging machinery.

## Error model

Three rules, in priority order.

**Never panic on untrusted input** (ruling 1). Every parser and decoder
returns `Result`; indexing is checked; arithmetic on file-derived values is
checked or saturating; recursion has explicit depth limits and decoders
have dimension and allocation budgets. These are hardening limits, not
conformance limits, and they are fuzz-enforced: fuzz builds run with
overflow checks on, and any panic — including a slice index or an allocator
abort — is a bug with a reduced fixture committed to the corpus.

**Errors are per-crate enums converging in the facade.** Each crate speaks
its own vocabulary with no upward dependencies; the facade defines one
public `Error` with a stable kind for callers that branch on codes like
"password required".

**A page that produces a value is a success, and its problems ride on the
value.** Real documents are broken in ways users cannot fix, so leniency
policy is repair-over-reject: hard errors are reserved for "no value can be
produced" (not a document, wrong password, cancelled, index out of range).
Everything else — a repaired xref, an unparseable annotation, a degraded
image — becomes a typed warning carried on the result (`Bitmap.warnings`,
`TextPage::warnings`), naming the object it touched (ruling 10). Warnings
are data, not log lines: tests assert on them, and hosts can surface them.

## Capability degradation

Some capabilities are deliberately absent, and the architecture makes
absence safe rather than pretending completeness: hitting one during
rendering draws a neutral placeholder in the object's rect and pushes a
warning naming the capability and the object (ruling 2). It never fails the
page and never panics. A missing codec is a quality problem, not a
correctness problem. What is absent is scheduled by corpus hit-rate
evidence, not ambition (ruling 3) — the current list, with each refusal's
reachability, lives in the [roadmap](ROADMAP.md) and in each feature doc's
"Refused by name" table.

## Concurrency

`Document` is `Arc`-shaped, `Send + Sync`, cheap to clone. The source bytes
are an immutable `Arc<[u8]>`; parsed objects are immutable `Arc` values in
a sharded cache. On a cache miss two threads may race to parse the same
object; both produce identical values because parsing is deterministic over
immutable bytes, so last-write-wins is correct and the only cost is a
wasted parse. `authenticate(&self)` takes a shared reference and stores the
outcome — including the user-versus-owner distinction — through interior
mutability, so "look at a page, then supply the password" works on a shared
document. **Concurrent reads and renders of one `Document` from many
threads are simply safe.**

Mutation is an exclusive copy-on-write `DocumentEditor`
([editing](features/editing.md)): the editor builds a new revision out of
new and borrowed `Arc`s while every open reader keeps its consistent
snapshot alive. Readers never observe a half-applied edit and never dangle.

On wasm32 the same types compile single-threaded; the library spawns no
threads and owns no global mutable state beyond once-cells, so threading is
entirely the embedder's business on every target.

## Outward rounding

Raster dimensions are `ceil` per axis of the scaled page box: A4
(595.276 × 841.89 pt) at 150 dpi renders to exactly **1240 × 1755**, never
1240 × 1754. A page must not lose its last row of pixels to rounding. This
is a documented API guarantee with a unit test pinning that exact case.
