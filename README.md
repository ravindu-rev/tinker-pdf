# tinker-pdf

A from-scratch, pure-Rust document engine. It reads, renders, extracts,
edits, writes and creates PDF; it opens CBZ, XPS and EPUB by synthesizing a
real PDF through its own ZIP, XML, CSS and layout engines. Every byte of
logic is in this repository — the inflate, the JPEG, JPEG 2000, JBIG2,
CCITT and PNG decoders, the crypto, the font parsers, the rasterizer, the
transcendental math. No `unsafe` code in any engine crate, and no
third-party crates outside dev/build/binding tooling — `cargo build` with
nothing but `rustup`, on Windows, macOS, Linux and
`wasm32-unknown-unknown`.

## The guarantees

- **It never panics on untrusted input.** Fuzz-enforced and release-gating:
  per-format fuzz targets, and a corpus of real-world documents opened
  without a crash.
  [docs/verification.md](docs/verification.md).
- **It is deterministic.** The same bytes render to bit-identical output
  on every target — committed fingerprints and document byte-hashes prove
  it, not a tolerance.
  [docs/features/determinism.md](docs/features/determinism.md).
- **It degrades honestly.** A page never silently fails: what cannot be
  drawn becomes a placeholder plus a typed warning naming the object, and
  what the engine cannot do it refuses *by name* — every feature doc
  carries its own "Refused by name" table.
  [docs/rulings.md](docs/rulings.md).
- **One public surface.** The `tinker-pdf` facade is the only crate users
  see; the C, Python, JavaScript/wasm, .NET, Go, Ruby and Java bindings
  project it 1:1 and add nothing of their own (a Swift package is source
  nobody has compiled yet).
  [docs/features/bindings.md](docs/features/bindings.md).

## Measured, not claimed

Under ruling 13 this engine's verification, like its implementation, is its
own: nothing outside this repository renders, parses, validates or measures a
document as evidence. [docs/verification.md](docs/verification.md) describes
how the engine is tested, and names the properties a first-party suite cannot
prove.

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

Runnable examples cover the rest, each with no arguments needed:

```sh
cargo run -p tinker-pdf --example open       # and render, extract, edit,
cargo run -p tinker-pdf --example create     # convert
```

See [crates/tinker-pdf/examples/](crates/tinker-pdf/examples/).

## What it does

| | |
| --- | --- |
| Read | [opening](docs/features/opening.md) · [filters](docs/features/filters.md) · [encryption](docs/features/encryption.md) · [document model](docs/features/document-model.md) · [fonts](docs/features/fonts.md) · [text](docs/features/content-and-text.md) |
| Render | [rasterizer](docs/features/rasterizer.md) · [rendering](docs/features/rendering.md) · [determinism](docs/features/determinism.md) |
| Write | [writing](docs/features/writing.md) · [editing](docs/features/editing.md) · [forms](docs/features/forms.md) · [creation](docs/features/creation.md) |
| Containers | [CBZ](docs/features/cbz.md) · [XPS](docs/features/xps.md) · [EPUB](docs/features/epub.md) |
| Embed | [bindings](docs/features/bindings.md) — C, Python, JavaScript/wasm, .NET, Go, Ruby, Java; Swift unverified |

Two honest limits worth knowing up front: the engine bundles no font faces
**by default** — a document that embeds none draws no text unless the host
supplies faces through `FontProvider`, or the `bundled-fonts` feature is on,
which carries twelve Liberation faces for hosts that have none — and its
pages are not compared against independent renderers at corpus scale. That,
and everything else it does not do yet, is ordered in
[docs/ROADMAP.md](docs/ROADMAP.md), with design docs for the major items
in [docs/design/](docs/design/).

## Workspace

The leaf crates (`filters`, `crypto`, `font`, `color`, `raster`, `math`,
`zip`, `xml`, `css`, `layout`, `pki`, `shape`, `svg`, `archive`) are
bytes-in, values-out, know nothing about PDF, and each is independently
fuzzable. `cos` owns file syntax; `content` interprets content streams and
emits to a `Device` trait — the text device needs no rasterizer; `render` is
the rasterizing device; `tinker-pdf` is the facade and the only crate users
see; `ffi` and `bindings/` sit on top.
[docs/architecture.md](docs/architecture.md) has the full map.

## Documentation

[docs/README.md](docs/README.md) indexes everything: one doc per feature,
the engineering rulings, the verification doc, the roadmap and the design
docs.

## License

MIT OR Apache-2.0, your choice.
