# tinker-pdf

A from-scratch, pure-Rust PDF engine: parse, decrypt, extract text, render,
write. The engine behind [Tinker](https://github.com/ravindu-rev/Tinker), and a
standalone library for Rust, JavaScript/wasm, Python and .NET.

> **Status: it reads and renders PDFs.** Opens damaged files, decrypts with
> correct security semantics, extracts text with geometry, renders vector
> graphics, images and gradients in colour, and writes valid documents —
> 1570 tests, four CI targets including wasm, with Python, JavaScript and .NET
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
pagination — larger on its own than everything built so far. None of the
three exists yet; each gets its own plan. The decision and its costs are in
[`docs/plans/gaps/28-tinker-integration-decisions.md`](docs/plans/gaps/28-tinker-integration-decisions.md).

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

Leaf crates (`filters`, `crypto`, `font`, `color`, `raster`) are bytes-in,
values-out, know nothing about PDF, and each is independently fuzzable.
`cos` owns file syntax; `content` interprets content streams and emits to a
`Device` trait — the text device needs no rasterizer, which is why text parity
lands before pixels exist; `render` is the rasterizing device; `tinker-pdf` is
the facade and the only crate users see; `ffi` and `bindings/` sit on top.

## License

MIT OR Apache-2.0, your choice. Unlike the engine it replaces, that also means:
embed it in anything.
