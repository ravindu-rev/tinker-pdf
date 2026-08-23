# tinker-pdf

A from-scratch, pure-Rust document engine. It reads, renders, extracts,
edits, writes and creates PDF; it opens CBZ, XPS and EPUB by synthesizing a
real PDF through its own ZIP, XML, CSS and layout engines. Every byte of
logic is in this repository — the inflate, the JPEG, JPEG 2000, JBIG2,
CCITT and PNG decoders, the crypto, the font parsers, the rasterizer, the
transcendental math. `#![forbid(unsafe_code)]` in every engine crate, and
no third-party crates outside dev/build/binding tooling — `cargo build`
with nothing but `rustup`, on Windows, macOS, Linux and
`wasm32-unknown-unknown`.

## The guarantees

- **It never panics on untrusted input.** Fuzz-enforced, release-gating:
  24 fuzz targets, 186 million recorded executions, and a 4 525-file
  corpus of real-world documents opened without one crash.
  [docs/verification.md](docs/verification.md).
- **It is deterministic.** The same bytes render to bit-identical output
  on every target — 15 committed fingerprints and 3 document byte-hashes
  prove it, not a tolerance.
  [docs/features/determinism.md](docs/features/determinism.md).
- **It degrades honestly.** A page never silently fails: what cannot be
  drawn becomes a placeholder plus a typed warning naming the object, and
  what the engine cannot do it refuses *by name* — every feature doc
  carries its own "Refused by name" table.
  [docs/rulings.md](docs/rulings.md).
- **One public surface.** The `tinker-pdf` facade is the only crate users
  see; the C, Python, JavaScript/wasm and .NET bindings project it 1:1 and
  add nothing of their own.
  [docs/features/bindings.md](docs/features/bindings.md).

## Measured, not claimed

As of August 2026: **2 940 tests** (0 failed) across 120 suites; corpus of
**4 525** documents with **4 484 rendering every page and zero crashes**, and
**4 225 of 4 225** rewrites of them validating against ISO 32000 read
strictly;
all 202 Adobe CMaps compiled in; UAX #14 line breaking at
**19 338 / 19 338** of Unicode's own conformance table.

Under ruling 13 this engine's verification, like its implementation, is its
own: nothing outside this repository renders, parses, validates or measures a
document as evidence. That is a decision with a price, and
[docs/verification.md](docs/verification.md) names the four properties it
costs rather than absorbing them.

## Quick tour

```rust
let bytes = std::fs::read("document.pdf")?; // or .cbz, .xps, .epub
let doc = tinker_pdf::Document::open(bytes)?;

for page in doc.pages() {
    let bitmap = page.render(&tinker_pdf::RenderOptions::at_dpi(150.0));
    let text = page.text().plain_text();
    // A page always produces a value; its problems ride on it.
    for warning in &bitmap.warnings { eprintln!("{warning:?}"); }
}

// Edit and save: fill a form, redact, reorder pages, then write an
// incremental update whose prefix is byte-identical to the original.
let mut editor = doc.editor();
```

## What it does

| | |
| --- | --- |
| Read | [opening](docs/features/opening.md) · [filters](docs/features/filters.md) · [encryption](docs/features/encryption.md) · [document model](docs/features/document-model.md) · [fonts](docs/features/fonts.md) · [text](docs/features/content-and-text.md) |
| Render | [rasterizer](docs/features/rasterizer.md) · [rendering](docs/features/rendering.md) · [determinism](docs/features/determinism.md) |
| Write | [writing](docs/features/writing.md) · [editing](docs/features/editing.md) · [forms](docs/features/forms.md) · [creation](docs/features/creation.md) |
| Containers | [CBZ](docs/features/cbz.md) · [XPS](docs/features/xps.md) · [EPUB](docs/features/epub.md) |
| Embed | [bindings](docs/features/bindings.md) — C, Python, JavaScript/wasm, .NET |

Two honest limits worth knowing up front: the engine bundles no font
faces — a document that embeds none draws no text unless the host supplies
faces through `FontProvider` — and while its output is proven stable and
valid, comparing its pages against independent renderers at corpus scale
is still owed. That, and everything else it does not yet do, is ordered in
[docs/ROADMAP.md](docs/ROADMAP.md), with design docs for the major items
in [docs/design/](docs/design/).

## Workspace

Ten leaf crates (`filters`, `crypto`, `font`, `color`, `raster`, `math`,
`zip`, `xml`, `css`, `layout`) are bytes-in, values-out, know nothing
about PDF, and each is independently fuzzable. `cos` owns file syntax;
`content` interprets content streams and emits to a `Device` trait — the
text device needs no rasterizer; `render` is the rasterizing device;
`tinker-pdf` is the facade and the only crate users see; `ffi` and
`bindings/` sit on top. [docs/architecture.md](docs/architecture.md) has
the full map.

## Documentation

[docs/README.md](docs/README.md) indexes everything: one doc per feature,
the engineering rulings, the verification doctrine, the roadmap and the
design docs.

## License

MIT OR Apache-2.0, your choice.
