# Bindings

Four surfaces over one facade: a C ABI, Python, JavaScript/WebAssembly and
.NET. Ruling 11 ([rulings.md](../rulings.md)) is the whole design — **the
facade is the only public surface**; a binding projects it 1:1 and adds no
logic, caching or defaults of its own. If a binding needs behaviour, the
facade grows it first, and then every binding has it.

## What it does

**The C ABI** (`tinker-pdf-ffi`). Handle-based and thread-safe because the
core is: a `tpdf_document` boxes a `Document`, which is `Send + Sync` and
cheap to clone, so handles may be used from any thread and freed
independently. Ownership is stated once: the engine allocates and the
matching `tpdf_*_free` releases; nothing crosses the boundary as a
caller-freed buffer, and a pointer into a handle's storage borrows it until
the handle is freed. Every call returns a `TpdfStatus` (`Ok`, `BadArgument`,
`NotAPdf`, `NeedsPassword`, `WrongPassword`, `NoSuchPage`, `NotEncrypted`,
`UnsupportedHandler`) and `tpdf_last_error_message` carries the detail.
Eighteen functions: `tpdf_version`, `tpdf_last_error_message`,
`tpdf_document_open` / `_free` / `_page_count` / `_is_encrypted` /
`_authenticate` (returning a `TpdfAuthLevel` of `None`, `User` or `Owner`) /
`_may_print` / `_set_fonts`, `tpdf_page_size` / `_text` / `_render`,
`tpdf_string_free`, and `tpdf_bitmap_width` / `_height` / `_stride` /
`_data` / `_free`. `#![forbid(unsafe_code)]` does not apply here — this is
the one crate whose job is the boundary — and `#![warn(missing_docs)]`
does.

**Python** (`bindings/python`, PyO3 directly over the facade — not through
the C ABI, which would add a second error translation for nothing).
`tinker_pdf.Document(bytes)`, `page_count`, `page_text(i)`, `render(i,
dpi=)` returning a bitmap whose `data` is a buffer (`memoryview` into numpy
or Pillow, zero-copy), `set_fonts(bytes)`. `render` and `page_text` release
the GIL, so a thread pool over pages is actually parallel. One wheel per
platform, not per interpreter: `abi3-py39`, and the release workflow
asserts the `abi3` tag is in the filename.

**JavaScript / wasm** (`bindings/js`, wasm-bindgen directly over the
facade). `PdfDocument`, `pageCount`, `isEncrypted`, `authenticate`,
`mayPrint`, `pageWidth` / `pageHeight`, `pageText(i)`, `setFonts`,
`renderPage(i, scale)`;
`bitmap.data()` copies, `bitmap.viewUnsafeUntilNextAllocation()` aliases
wasm linear memory and silently becomes zero-length when a later allocation
grows the memory — observed, not hypothesised: `node_smoke.mjs` renders,
takes a view, renders larger and asserts the view's length is 0. The
dangerous call has the warning in its name. **ESM only, `--target web`**:
the `nodejs` target's CommonJS loads the `.wasm` with a synchronous read a
browser cannot do, so a dual package would be two builds of the engine that
can diverge, and ruling 11's point is that a binding has nothing to diverge
*with*. Node ≥ 18 runs the same file by handing `init` the bytes. The
package is `tinker-pdf-js`, name and version derived from `Cargo.toml` so
`cargo xtask versions` has no fifth manifest to police. The `.wasm` is
2.03 MB, 1.40 MB gzipped, with all 202 predefined CMaps in; `cmap-predefined`
off is the switch for a host that renders no CJK.

**.NET** (`bindings/dotnet`, C# over the C ABI). Every native handle lives
in a `SafeHandle`, so a document or bitmap is released exactly once even if
an exception unwinds past it. `Document.Open(bytes)`, `PageCount`,
`PageText(i)`, `Render(i, scale)` with `bitmap.Pixels` as a
`ReadOnlySpan<byte>` valid while the bitmap lives, `SetFonts(bytes)`. The
P/Invoke declarations are written out, so the binding builds with nothing
but the .NET SDK. A NuGet package carries `runtimes/<rid>/native/`;
`cargo xtask nuget-stage` maps the host to its RID with a unit test, because
a package built with the wrong RID restores, compiles and throws
`DllNotFoundException` on first use, and `dotnet pack` on an empty
`runtimes/` produces a perfectly valid managed-only package.

**`set_fonts` everywhere.** The engine bundles no faces and reads no font
directories ([fonts](fonts.md)), so a document that embeds none extracts its
text perfectly and draws none of it. The `FontProvider` seam is projected
across all four surfaces, and every smoke test renders *twice* — blank
without a face, inked with one — because "a bitmap of the right size came
back" passes on a build whose renderer does nothing at all.

**Packaging, built and dry-run, nothing published.** `cargo run -p xtask --
release` walks wheel, npm package, NuGet package and crates in an order
computed from the manifests. The dry run is the default and `--execute` is
the flag that publishes, because a half-published release cannot be
retracted from crates.io. On Windows/x86_64 all four were built, installed
and rendered through, each producing the same **1 190 inked pixels** — the
evidence they are projections of one engine. **Nothing has been published
to any registry**, deliberately: the facade does not freeze until 0.1.0 and
a published package invites dependence on an API that is explicitly
unstable. The exercise found three defects nothing else could — a crate
excluding the CMap registry its own `build.rs` requires, workspace
dependencies without `version` beside `path`, and the managed-only NuGet
package above.

## API

```python
import tinker_pdf
doc = tinker_pdf.Document(open("file.pdf", "rb").read())
doc.set_fonts(open("DejaVuSans.ttf", "rb").read())
bitmap = doc.render(0, dpi=150.0)
memoryview(bitmap.data)
```

```js
import init, { PdfDocument } from 'tinker-pdf-js';
await init();
const doc = new PdfDocument(bytes);
const bitmap = doc.renderPage(0, 1.0);
const pixels = bitmap.data();          // a copy; safe to keep
```

```csharp
using var document = Document.Open(File.ReadAllBytes("file.pdf"));
document.SetFonts(File.ReadAllBytes(@"C:\Windows\Fonts\arial.ttf"));
using var bitmap = document.Render(0, scale: 2.0);
ReadOnlySpan<byte> pixels = bitmap.Pixels;
```

```c
TpdfDocument *doc = NULL;
if (tpdf_document_open(bytes, len, &doc) == 0 /* TpdfStatus::Ok */) {
    TpdfBitmap *bm = NULL;
    tpdf_page_render(doc, 0, 2.0, 2 /* TpdfPixelFormat::Rgb8 */, &bm);
    /* tpdf_bitmap_width(bm), tpdf_bitmap_stride(bm), tpdf_bitmap_data(bm, ...) */
    tpdf_bitmap_free(bm);
    tpdf_document_free(doc);
}
```

(The crate ships no generated header; the `#[repr(C)]` enums and the
`extern "C"` signatures in `crates/tinker-pdf-ffi/src/lib.rs` are the
contract, and the .NET binding's P/Invoke declarations are a worked
transcription of them.)

Each binding's README ([js](../../bindings/js/README.md),
[python](../../bindings/python/README.md),
[dotnet](../../bindings/dotnet/README.md)) carries its build, smoke-test and
packaging commands.

## Refused by name

| What | How it shows | Why | See |
| --- | --- | --- | --- |
| Editing, forms, creation, saving | not present on any binding — the surface is open, page count, encryption/auth, permissions, page size, text, render, `set_fonts` | the write surface has not been projected; the facade shape is already the design | [ROADMAP.md](../ROADMAP.md) (design/bindings-write.md) |
| Outline, links, metadata, attachments, XMP, warnings | not projected | same — read surface beyond rendering and text is owed | [ROADMAP.md](../ROADMAP.md) |
| CommonJS build | none; ESM only | two builds of the engine can diverge | — |
| Holding a wasm `view()` across an engine call | the view becomes zero-length | wasm memory growth detaches the buffer; use `data()` | — |
| A security handler the engine lacks | `TpdfStatus::UnsupportedHandler` | public-key encryption is absent | [encryption](encryption.md) |
| Published packages | `pip install` / `npm install` / `dotnet add package` do not work yet | the facade is unstable until 0.1.0 | [ROADMAP.md](../ROADMAP.md) |

## Verified

- `crates/tinker-pdf-ffi`'s unit tests drive the boundary end to end: a
  document opens and reports its pages, page size and text cross, rendering
  crosses with its pixels, authentication reports which password matched,
  null and nonsense arguments are refused rather than dereferenced, a page
  past the end is reported, the version string is readable, and a supplied
  face reaches the C ABI (with the no-regular-face and null-document
  refusals). The crate type-checks in CI on every commit (the bindings
  and fuzz crates are outside the workspace, so CI checks them explicitly —
  four fuzz targets once failed to compile for months because nothing did).
- Smoke tests run against an *installed* artifact, never the source tree:
  `bindings/python/tests/wheel_smoke.py` against a `pip install`ed wheel,
  `bindings/js/tests/node_smoke.mjs` against an `npm install`ed tarball,
  `bindings/dotnet/tests/Smoke` against a local folder feed with the source
  list cleared so a missing package fails rather than resolving from
  nuget.org. Each asserts the blank-then-inked render.
- `bindings/js/demo/verify.mjs` drives the browser demo in headless
  Chromium and checks the ink's bounding box is the shape of a line of
  text rather than a smear or a stray pixel.
- The release workflow asserts the `abi3` wheel tag and greps the `.nupkg`
  for all three RIDs; every Linux and macOS leg of it is written and
  unobserved ([ROADMAP.md](../ROADMAP.md) Tier 1).
