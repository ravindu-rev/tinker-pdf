# Architecture

tinker-pdf is a from-scratch, pure-Rust document engine: a workspace of
crates in which every byte of logic is this repository's own — the inflate,
the image codecs, the crypto, the font parsers, the rasterizer, the XML
parser, the CSS engine, the layout engine, and the transcendental math they
all share. No engine crate's library contains `unsafe` code, and
`cargo-deny` denies by name the crates that would implement any of it.

## Locked decisions

These are given; this document states each boundary, not the argument.

- **License:** MIT OR Apache-2.0, dual.
- **Everything hand-rolled.** All document logic and all primitives are
  ours. The only exemption is dev/build/binding tooling that never ships in
  a user's artifact and never touches document bytes at runtime: `proptest`,
  `criterion`, `cargo-fuzz`, `PyO3`, `wasm-bindgen`, `maturin`, `csbindgen`.
  Anything else — including "just a small helper" like a hash or a
  bit-reader — is out, and `cargo-deny` fails the build on such crates by
  name (`deny.toml`). The policy lives in CI, not in review vigilance.
- **`wasm32-unknown-unknown` is a first-class target.** No C toolchain
  exists anywhere, so the wasm build is a plain `cargo build`. The facade
  takes bytes, never paths, for the same reason.
- **Spec baseline:** PDF 1.7 (ISO 32000-1). PDF 2.0 deltas that matter
  early are tracked in [pdf20-deltas.md](pdf20-deltas.md); full 2.0
  conformance is [ROADMAP](ROADMAP.md) row SD-18.
- **Formats: PDF, and documents that open as one.** CBZ and the other comic
  archives, XPS, EPUB, and one-file SVG, HTML, FB2 and image documents open
  as a `Document` by synthesizing a real PDF at `open`, and Markdown through
  `Document::open_markdown`, so every downstream capability applies to all
  of them ([cbz](features/cbz.md), [xps](features/xps.md),
  [epub](features/epub.md), [opening](features/opening.md)). Every further
  input format is a row of the [ROADMAP](ROADMAP.md)'s formats-in tier (FI).
- **API stability:** everything is 0.x; the facade freezes at 0.1.0.
  Internal crates never gain stability promises at all.

## Crate DAG

Each crate with the internal crates it depends on:

```text
tinker-pdf-math
tinker-pdf-filters
tinker-pdf-crypto
tinker-pdf-xml
tinker-pdf-css
tinker-pdf-color    → math
tinker-pdf-raster   → math, color
tinker-pdf-font     → filters
tinker-pdf-zip      → filters
tinker-pdf-archive  → filters
tinker-pdf-pki      → crypto
tinker-pdf-shape    → font
tinker-pdf-layout   → css
tinker-pdf-svg      → xml, css, math

tinker-pdf-cos      → filters, crypto, font, pki, shape
tinker-pdf-content  → cos, font, color
tinker-pdf-render   → content, raster, font, color
tinker-pdf          → every crate above but math
tinker-pdf-ffi      → tinker-pdf

tools: pdfcmp, tpdf (both on the facade and nothing below it)
```

**The leaf crates** — `filters`, `crypto`, `font`, `color`, `raster`,
`math`, `zip`, `xml`, `css`, `layout`, `pki`, `shape`, `svg`, `archive` — are
bytes-in/values-out with zero PDF types (ruling 8 defines a leaf; the
definition binds, not the list). `archive` is last rather than next to
`zip` because the ordinals in `xtask`'s `ALLOWED` table are positions in
this list. This is the property that makes each one independently fuzzable:
a fuzz target hands `tinker-pdf-font` a byte slice and expects a value or a
structured error, with no COS machinery in the corpus or the crash triage.
It also means a leaf is tested against its own spec (DEFLATE against
RFC 1951, CFF against Adobe TN 5176) without a PDF in sight.

The leaf-to-leaf edges each point from a higher layer down:
`font → filters` (the CMap asset pipeline), `zip → filters` (raw DEFLATE
and CRC-32 — and **not** `zip → archive` for ZIP methods 14, 12 and 93:
`Archive::read_coded` takes the decoder as a callback and the facade, which
already depends on both, hands `tinker_pdf_archive::lzma`,
`tinker_pdf_archive::bzip2` and `tinker_pdf_archive::zstd` in, so the methods
that need a second leaf's decoder cost an argument rather than an edge),
`archive → filters` (the same two, for the same two reasons — 7z method
040108 *is* RFC 1951, and 7z and RAR both record a per-file CRC-32),
`layout → css` (computed styles in, boxes out), `pki → crypto` (RFC 5280's
key identifier is a SHA-1, and DER stays out of the cipher crate because the
two fail differently — a wrong number caught by published vectors, against a
panic or an overread on untrusted structure), `shape → font` (`Sfnt` parses
the table directory, and the OpenType Layout tables are read in `shape`
because `font`'s charter is the tables *metrics* need — a lookup is not a
metric), `svg → xml` and `svg → css` (an SVG *is* XML, and its presentation
attributes, `style=""` and `<style>` are all CSS values), and
`raster → color` (a `/Lab` transparency group composites in Lab, 11.4.7, so
`Canvas` puts an sRGB colour into that space and reads one back with the
colour crate's matrices rather than a copy of them).

**An edge into `math` is not counted as leaf-to-leaf**: `color → math`,
`raster → math` and `svg → math` exist, and `tinker-pdf-math` is the bottom
of the graph rather than a peer — `xtask`'s `ALLOWED` table calls the row
above it *“nothing internal beyond the maths”* — so depending on it is the
baseline every leaf starts from and not a coupling anyone has to argue for.

**Three** of `cos`'s edges into a leaf need an argument — its full set is
`filters`, `crypto`, `font`, `shape`, `pki` — and the first two make the
same argument:

- `cos → font` — reading a font *dictionary* (`/Encoding`, `/ToUnicode`,
  standard-14 metrics) is object-model work that needs the leaf's CMap parser
  and encoding tables, and a crate whose only job is to hold two tables would
  be worse.
- `cos → pki` — reading a `/Recipients` envelope (7.6.5) is object-model work
  that needs a DER parser, and installing a decryptor is `CosDocument`'s own
  operation.

The third is a different argument and the only one of the three that a
reader might expect *not* to be here:

- `cos → shape` — filling a form field. `fill.rs` is the one producing path
  in the tree whose entry point takes a string and no glyphs:
  `DocumentBuilder::glyph_run` takes glyphs the caller positioned, so the
  seam is already in its signature and shaping sits *above* cos for it,
  but `DocumentEditor::set_field_value(name, value)` has nowhere to put
  one. Without the edge, `fill.rs` would write `?` for every character above
  the single-byte range.

  A `dyn Shaper` on `DocumentEditor` filled from the facade fails twice on
  its own terms. The facade **re-exports `DocumentEditor` verbatim** rather
  than wrapping it, so a seam only the facade filled would leave
  `tinker_pdf_cos::DocumentEditor` — a published crate's public API — still
  writing `?` for Arabic while the identical re-export did not: two answers
  to one question. And a seam the *caller* fills leaves the default broken,
  which is the defect the edge closes.

  What it does not cost: it points down into a leaf, the direction ruling 8
  allows without argument, and cos hands it face bytes and a `&str`, so
  `tinker-pdf-shape` learns no PDF vocabulary. `tinker-pdf-content` and
  `tinker-pdf-render` have no edge to it, and cos interprets no content
  streams at all. The only text it shapes is an appearance it built itself.

`cos` is not a leaf, so none of the three is leaf-to-leaf however useful
it is.

The graph cannot cycle, because `filters` depends on nothing.

`tinker-pdf-cos` owns file syntax, xref, repair, the writer, and the strict
validator that reads a file back with the repairs turned off (ruling 13,
[verification](verification.md)).
`tinker-pdf-content` is the content-stream interpreter plus `trait Device`,
and ships two devices: the text device, and the recording device that keeps
every call in order for the consumers a display list, an SVG writer, a
structured-text serialiser, a table reconstructor, a reading-order inference
and a glyph-usage walk each need. The rasterizing device lives in
`tinker-pdf-render`. Because the drawing device is in a different crate, the
text-extraction path never links a rasterizer — the seam is load-bearing,
not decorative (ruling 7). `tinker-pdf` is the facade and the only
user-facing crate (ruling 11).

Dependency direction is enforced: `cargo xtask dag` diffs `cargo metadata`
against the declared graph in CI and fails on any new edge. It exists
because nothing else can catch this — an undeclared edge compiles.
Convenient shortcuts between crates are how seams die.

## Per-crate map

| Crate | Role | Feature doc | Fuzz targets |
| --- | --- | --- | --- |
| `tinker-pdf` | facade; the only public surface | all of [features/](README.md) | `render_page`, `standalone`, `fb2`, `markdown`, `form_data`, `signatures` |
| `tinker-pdf-cos` | file syntax, object store, writer, strict validator | [opening](features/opening.md), [document-model](features/document-model.md), [writing](features/writing.md), [forms](features/forms.md), [creation](features/creation.md) | `cos_document`, `cos_object`, `form_script`, `annotation_appearance` |
| `tinker-pdf-filters` | stream filters + image codecs | [filters](features/filters.md) | `ascii_filters`, `bmp`, `brotli`, `ccitt`, `gif`, `inflate`, `jbig2`, `jpeg`, `jpx`, `jxr`, `lzw`, `png`, `tiff`, `webp` |
| `tinker-pdf-crypto` | ciphers, hashes, security handlers, RSA/ECDSA verify | [encryption](features/encryption.md) | `crypt`, `crypt_ciphers` |
| `tinker-pdf-pki` | DER (X.690), X.509 (RFC 5280), CMS (RFC 5652) | [signatures](features/signatures.md) | `pki_der`, `pki_cms` |
| `tinker-pdf-shape` | OpenType Layout: GDEF, GSUB, GPOS | [design/shaping.md](design/shaping.md) | `shape`, `shape_text` |
| `tinker-pdf-font` | font and CMap parsing, subsetting | [fonts](features/fonts.md) | `cff`, `cff_subset`, `cmap`, `sfnt`, `truetype`, `type1`, `woff` |
| `tinker-pdf-content` | interpreter + `Device` seam, text device, recording device | [content-and-text](features/content-and-text.md) | `content_tokenizer` |
| `tinker-pdf-raster` | deterministic AA rasterizer | [rasterizer](features/rasterizer.md) | — (driven via `render_page`) |
| `tinker-pdf-render` | the rasterizing `Device` | [rendering](features/rendering.md) | — (driven via `render_page`) |
| `tinker-pdf-color` | colour spaces and functions | [rendering](features/rendering.md) | `icc_profile` |
| `tinker-pdf-math` | pinned transcendentals, `no_std` | [determinism](features/determinism.md) | — |
| `tinker-pdf-zip` | ZIP reader | [cbz](features/cbz.md) | `zip_archive` |
| `tinker-pdf-archive` | the containers that are not ZIP: tar, 7z, RAR | [cbz](features/cbz.md), [design/comic-archives.md](design/comic-archives.md) | `tar`, `sevenz`, `rar`, `bzip2`, `ppmd`, `zstd` |
| `tinker-pdf-xml` | XML pull parser; WHATWG HTML tokenizer and tree builder; the Encoding Standard's single-byte decoders | [xps](features/xps.md), [opening](features/opening.md) | `xml`, `html` |
| `tinker-pdf-css` | CSS engine | [epub](features/epub.md) | `css` |
| `tinker-pdf-svg` | SVG 1.1: markup in, a display list out | [epub](features/epub.md) | `svg` |
| `tinker-pdf-layout` | box model, fragmentation, line breaking | [epub](features/epub.md) | `layout` |
| `tinker-pdf-ffi` | C ABI | [bindings](features/bindings.md) | — |

Tools: `tpdf` (the CLI over the facade, whose `render` writes a `.png` a
page through `Bitmap::to_png` and whose writing commands save through
`write::save`) and `pdfcmp` (perceptual comparator), both
described in [verification.md](verification.md) and both on the facade and
nothing below it — `xtask`'s `TOOLS` table enforces that, so a tool exercises
what a user gets rather than reaching past the API into a leaf. `xtask`
holds the workspace police (`dag`, `libm`, `oracles`, `vendor`, `versions`,
`check`) and the release, corpus and packaging machinery.

### `tinker-pdf-archive`: why a second archive crate, and why no trait

The comic path reads four containers, and the three that are not ZIP live
in their own leaf rather than in `tinker-pdf-zip`. The argument is a
measurement, not a taste, and it is worth stating here because the obvious
shape — one archive crate with a `trait Container` over all four — is the one
this rejects.

`tinker_pdf_zip::Archive::read` returns a `Cow` and hands a **stored entry back
borrowed**, copied nowhere, and that crate's own suite pins it. The comic path
places image bytes into a PDF stream verbatim, so a copy per entry is a copy
of the whole archive, and a 200-page scan is the size at which that stops
being a detail. tar keeps the property and strengthens it: every entry is a
contiguous range of the input, so its `read` returns a plain `&[u8]`.

**7z cannot have it.** A solid block decodes many files from one LZMA stream,
so no range of the input is any one file and the read must return owned bytes.
A trait over all four would have to return the weakest of the four signatures,
which deletes the exact property that ZIP test exists to hold — in the crate
that has it, to give four unrelated readers one name. `tinker-pdf-zip`'s
charter is APPNOTE 6.3.10 and nothing else ([cbz](features/cbz.md), and
ruling 8's own example), and a crate whose charter names one specification is
not where three more go.

So: **one crate, three container modules, three error enums, three entry
types, and no trait over them.** What tar, 7z and RAR share is a negative —
the archive containers that are not ZIP — which is weaker than `filters`'
"decoders" and is honestly weaker. It is still real, and it buys one node and
one edge instead of three of each. Where the four converge is the facade,
which is where they have to: deciding what a *page* is needs document types,
and a leaf may not have them.

The crate takes no edge to `tinker-pdf-crypto` for the AES-256 that 7z and
RAR 5 both offer: encrypted archives are refused by name in both, and a
comparison of two bytes already gives that refusal. Nor to
`tinker-pdf-math`, which nothing here needs — an LZMA range decoder and a
Huffman table are integer arithmetic end to end, which is also what makes
ruling 4 free in this crate rather than something it has to be careful about.

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

Some capabilities are absent, and the architecture makes absence safe
rather than pretending completeness: hitting one during rendering draws a
neutral placeholder in the object's rect and pushes a warning naming the
capability and the object (ruling 2). It never fails the page and never
panics. A missing codec is a quality problem, not a correctness problem.
Each absent capability is a roadmap row, ordered by corpus hit-rate evidence
(ruling 3) — the current list, with each refusal's reachability, lives in the
[roadmap](ROADMAP.md) and in each feature doc's "Refused by name" table.

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

**The decision that follows from that: the pools live outside the facade,
and there is no threading API on `tinker-pdf`.** No `render_all`, no job
count, no feature flag, no runtime — the library's whole contribution to
concurrency is that `Document` is `Send + Sync`, that `Document::page`
returns an *owned* `Page` (it clones an `Arc` rather than borrowing, so a
worker holding a page borrows nothing and a pool over page indices needs no
lifetime work), and that `FontProvider` is `Send + Sync` too. Given those
three, a pool is a dozen lines of `std::thread::scope` in the caller — and
the same dozen lines in here would be code that cannot exist on wasm32 and
that every embedder with an executor of its own would have to be talked out
of. Ruling 11 says the facade is the only public surface; this is that
surface staying small where the alternative is a second scheduler. A pool
on the facade, serial by default, is [ROADMAP](ROADMAP.md) row P-15.

Three things exercise the claim:

- `crates/tinker-pdf/tests/tinker_parity.rs`'s
  `one_document_is_read_and_rendered_by_several_threads_at_once` shares
  **one** `&Document` across four scoped threads — `Sync`, not the `Send`
  that a clone per thread would have shown — and each walks every page,
  reading its text and rendering it, against the serial answer page by page.
- `crates/tinker-pdf/examples/parallel.rs` is those dozen lines, as an
  embedder would write them, checking every page from the pool byte for byte
  against the same page drawn serially. CI runs it.
- `tpdf render --jobs N` is the same pool in the command-line tool, with the
  property that makes it usable: results are buffered per page and printed
  in page order after the join, so `--jobs 8` and `--jobs 1` write the same
  bytes. The flag changes the clock and nothing else, and defaults to 1.

## Outward rounding

Raster dimensions are `ceil` per axis of the scaled page box: A4
(595.276 × 841.89 pt) at 150 dpi renders to exactly **1240 × 1755**, never
1240 × 1754. A page must not lose its last row of pixels to rounding. This
is a documented API guarantee with a unit test pinning that exact case.
