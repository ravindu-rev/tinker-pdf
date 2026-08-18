# tinker-pdf

A from-scratch, pure-Rust PDF engine: parse, decrypt, extract text, render,
write. The engine behind [Tinker](https://github.com/ravindu-rev/Tinker), and a
standalone library for Rust, JavaScript/wasm, Python and .NET.

> **Status: it reads and renders PDFs.** Opens damaged files, decrypts with
> correct security semantics, extracts text with geometry, renders vector
> graphics, images and gradients in colour, and writes valid documents —
> 1872 tests, four CI targets including wasm, with Python, JavaScript and .NET
> bindings. It has met a real corpus: 4 525 documents from pdf.js, veraPDF,
> qpdf and the PDF Association, 4 484 of them rendering every page, **not one
> crash**. The two things it still cannot do: draw text for documents that do
> not embed their fonts (it ships no substitute face — the host supplies them
> through `FontProvider`), and compare a page it drew against a page anything
> else drew. [`docs/STATUS.md`](docs/STATUS.md) is the honest ledger;
> [`docs/PLAN.md`](docs/PLAN.md) is the full design.

## Why another PDF engine

Tinker was built on MuPDF and documented every scar in
[`Tinker/docs/mupdf-limitations.md`](https://github.com/ravindu-rev/Tinker/blob/main/docs/mupdf-limitations.md):
a vendored patch just to compile on Windows, a permissions API that reports
every document as unrestricted, owner and user passwords indistinguishable,
linearized output removed upstream, signing/subsetting/flatten unexposed, and
an AGPL license that follows every embedder. tinker-pdf answers each of those
by design:

- **Pure Rust.** Builds with `cargo` alone on Windows/MSVC, macOS, Linux and
  `wasm32-unknown-unknown`. No C toolchain, no bindgen, no vendored anything.
- **Correct security semantics.** Raw `/P` preserved with typed accessors;
  `authenticate` reports user vs owner; constant-time password checks.
- **Send + Sync reads.** Concurrent renders of one `Document` from many
  threads — no actor model imposed on callers.
- **Writing is first-class.** Incremental updates (byte-identical prefix, the
  foundation for signing), full rewrites, encryption on save, linearization.
- **Honest degradation.** Rare filters (JBIG2, JPX) ship as capability flags
  with placeholder rendering and warnings — a page never silently fails.
- **MIT OR Apache-2.0.** Embeddable anywhere, including app stores.

**Everything is hand-rolled**: the inflate, the JPEG decoder, the font parsers,
the rasterizer, the crypto. No third-party crates for PDF logic or primitives —
dev/build/binding tooling only. That is a deliberate, documented choice; see
[`docs/plans/00-architecture.md`](docs/plans/00-architecture.md).

**PDF is where it starts, and no longer where it stops.** This engine was
described as a PDF engine "and always will be" until 16 August 2026, when the
owner decided that the three formats MuPDF also opened — **CBZ, XPS and
EPUB** — are to be built here rather than dropped, converted or left to
MuPDF. That is what finally removes the AGPL dependency without losing a
format. It is honest about the size: CBZ is small, XPS is substantial, and
EPUB is a layout engine — a CSS cascade, a box model, line breaking and
pagination — larger on its own than everything built so far. Each gets its own
plan. The decision and its costs are in
[`docs/plans/gaps/28-tinker-integration-decisions.md`](docs/plans/gaps/28-tinker-integration-decisions.md).

**Two of the three are built.** A `.cbz` — a ZIP of page images — opens as
a `Document` whose pages are its images, in the order a reader expects, at the
image's own pixel size. Nothing about it is special below the facade: the
archive is turned into a real PDF at `open`, so every capability the engine
already has arrives with it, and `Document::cos()` hands back a document qpdf
reads clean. JPEG and PNG are read and **everything else is refused by name** —
a `.cbr`, a `.cb7`, a GIF page, an encrypted entry. The ZIP reader and the PNG
decoder are ours, like everything else here; `deny.toml` names the crates that
would have made them somebody else's.
[`docs/plans/gaps/29-cbz.md`](docs/plans/gaps/29-cbz.md) is the plan and its
record.

**And an `.xps` opens as a fixed document.** Not as a comic, which is what it
used to do: one ZIP signature covers CBZ, XPS, EPUB, ODF and every JAR ever
built, so until August 2026 a real XPS carrying a picture opened as a one-page
comic *whose page was the picture*, with the text, the fonts and the page size
discarded and no warning at all. Now ECMA-388 E.3's own three-step test decides
which it is, an OPC package layer resolves parts and relationships, and a
`FixedDocumentSequence` is paged in its markup's order at the size its markup
states. Paths, glyphs, images and five brushes reach the page, through an XML
parser that is an **eighth leaf crate** and **refuses DTD content by name**
rather than bounding it — which is what makes billion laughs a named refusal
instead of a budget. Everything else is refused by name: `VisualBrush`,
signatures, print tickets, 3D, TIFF and JPEG XR. Nothing in this repository
wrote any of the eight packages it is tested against.
[`docs/plans/gaps/30-xps.md`](docs/plans/gaps/30-xps.md) is the plan and its
record; EPUB is not built.

## The plan

| | |
| --- | --- |
| [**Status**](docs/STATUS.md) | **What is built and what is not** |
| [Master plan](docs/PLAN.md) | Phases, dependency lanes, checkpoints, off-ramps |
| [00 Architecture](docs/plans/00-architecture.md) | Crate DAG, policies, concurrency model |
| [01 COS & objects](docs/plans/01-cos-and-object-model.md) · [02 Filters](docs/plans/02-filters.md) · [03 Encryption](docs/plans/03-encryption.md) | The file format |
| [04 Document semantics](docs/plans/04-document-semantics.md) · [05 Fonts](docs/plans/05-fonts.md) · [06 Content & text](docs/plans/06-content-and-text.md) | Reading — to text parity |
| [07 Rasterizer](docs/plans/07-rasterizer.md) · [08 Rendering device](docs/plans/08-rendering-device.md) | Pixels |
| [09 Writing](docs/plans/09-writing.md) · [10 Editing](docs/plans/10-editing.md) · [11 Forms](docs/plans/11-forms.md) · [12 Creation](docs/plans/12-creation.md) | Mutation |
| [13 Bindings](docs/plans/13-bindings.md) · [14 Testing & corpora](docs/plans/14-testing-and-corpora.md) · [15 Tinker integration](docs/plans/15-tinker-integration.md) | Shipping |
| [99 Consistency](docs/plans/99-consistency.md) | Cross-phase rulings — these override the phase plans |

## Workspace

Eight leaf crates (`filters`, `crypto`, `font`, `color`, `raster`, `math`,
`zip`, `xml`) are bytes-in, values-out, know nothing about PDF, and each is
independently fuzzable. `cos` owns file syntax; `content` interprets content
streams and emits to a `Device` trait — the text device needs no rasterizer,
which is why text parity lands before pixels exist; `render` is the rasterizing
device; `tinker-pdf` is the facade and the only crate users see; `ffi` and
`bindings/` sit on top.

## License

MIT OR Apache-2.0, your choice. Unlike the engine it replaces, that also means:
embed it in anything.
