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
`UnsupportedHandler`, `NoSuchSignature`) and `tpdf_last_error_message`
carries the detail. Those numbers *are* the ABI — a C caller compares them
against literals and the .NET binding against an `int` — so 0–7 are frozen
and `NoSuchSignature` was **appended** at 8 rather than inserted; a unit
test pins all nine one by one, and another pins every discriminant of the
six signature enums, because those are transcribed by hand into
`bindings/dotnet/TinkerPdf.cs`.

**Forty-eight functions.** Eighteen open and render: `tpdf_version`,
`tpdf_last_error_message`, `tpdf_document_open` / `_free` / `_page_count` /
`_is_encrypted` / `_authenticate` (returning a `TpdfAuthLevel` of `None`,
`User` or `Owner`) / `_may_print` / `_set_fonts`, `tpdf_page_size` /
`_text` / `_render`, `tpdf_string_free`, and `tpdf_bitmap_width` /
`_height` / `_stride` / `_data` / `_free`.

Thirty read signatures (12.8). Reading only: the signing side is not
projected and is not coming here, because a `Signer` is a host callback
and callbacks across this boundary are `design/bindings-write.md`'s to own.
`tpdf_document_signatures` hands back an opaque `TpdfSignatures` on the
`TpdfBitmap` pattern — the engine's own copy, so it outlives the document
— with `tpdf_signatures_count` / `_free` and, per index,
`tpdf_signature_field_name` / `_sub_filter` / `_reason` / `_location` /
`_name` (strings freed with `tpdf_string_free`, and **null on `Ok` means
the dictionary has no such entry**, which is why a wrong index is
`NoSuchSignature` and not a null), `_coverage` (`TpdfCoverage`:
`WholeFile`, `Revision`, `Suspicious`), `_covers_whole_file`,
`_is_usage_rights`, `_certification_level` (1–3, **0 for none**),
`_span_count` and `_span`. Trust anchors arrive one at a time —
`tpdf_trust_anchors_new` / `_add` / `_count` / `_free` — rather than as an
array of pointers and lengths, because `_add` refuses bytes that are not a
certificate *at the moment they are offered* and an array could only report
that as one aggregate failure. `tpdf_document_verify_signatures` takes
those anchors, a `judge_validity` flag and an `i64` instant — two arguments
because the facade's `Option<i64>` has no C spelling — and returns
`TpdfVerdicts` (`_count` / `_free`) with `tpdf_verdict_cms_state`,
`_document_digest`, `_signature_check`, `_chain`, `_signer_subject`,
`_signer_issuer`, `_signer_validity`, `_weakness_count` and `_weakness`.
There is no `is_valid` and there will not be one: the four questions are
four `#[repr(C)]` enums, and `NotChecked` is not `Differs`.

`#![forbid(unsafe_code)]` does not apply here — this is the one crate whose
job is the boundary — and `#![warn(missing_docs)]` does.

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
`ReadOnlySpan<byte>` valid while the bitmap lives, `SetFonts(bytes)`,
`ReadSignatures()` and `VerifySignatures(anchors, at)` — the last taking a
`TrustAnchors` it will not default for you and a `long?` instant whose
`null` is the flag the C ABI spells separately. The P/Invoke declarations
are written out, so the binding builds with nothing but the .NET SDK; one
of them, `tpdf_verdict_signer_validity`, is declared `ref` rather than
`out` because it returns a flag rather than a status and writes nothing
when the flag is 0, and an `out` would leave the caller reading stack
rubbish the compiler believed assigned. A NuGet package carries `runtimes/<rid>/native/`;
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

**And ten of the fifteen crates had never been in it.** `cargo publish
--dry-run` resolves dependencies against the live index, and nothing named
`tinker-pdf-*` has ever been published there, so every crate above the five
leaves failed with `no matching package named tinker-pdf-crypto` — which reads
exactly like a broken manifest and is not one. The dry run's answer was to
report those steps as unprovable and carry on, so "the pipeline has been
exercised end to end" covered a third of it.

`cargo run -p xtask -- release --local-registry` closes that. Each crate's
registry dependencies are patched at `target/package/<name>-<version>/` — the
unpacked archive the previous step left behind — so every crate is *verified
against the same bytes its dependents would download*, which is nearer to a
real publish than building against this checkout would be. The patch set is
everything published before that crate: one level deep is not enough, because a
packaged dependency has registry dependencies of its own, and the whole
workspace is too much, because nothing is packaged when the first crate runs.
Both were tried, and both failures are written into the test that pins the
width. The whole pipeline now reports **23 of 24 steps run, 0 unprovable** —
the one skip is `dotnet nuget push`, which has no harmless form.

**Observed, on one tag, 24 August 2026.**
[Run 32690957039](https://github.com/ravindu-rev/tinker-pdf/actions/runs/32690957039)
at `bf1630b`: thirteen jobs, twelve green and `publish` skipped, which is what
a tag push is supposed to do — publishing needs a deliberate
`workflow_dispatch` carrying `publish: true` and a tag cannot reach it. The
`crates` job packaged and verified all fifteen crates on Linux, reporting `15
step(s) ran, 0 skipped, 0 unprovable`. The collector counted what came out:

```
wheels=3 (abi3=3) wasm=1 nupkg=1
RELEASE-ARTEFACTS: ALL FOUR PRESENT
```

Three abi3 wheels — `manylinux_2_17_x86_64`, `win_amd64`, `macosx_11_0_arm64`
— one npm package carrying `tinker_pdf_js_bg.wasm`, one `TinkerPdf.0.0.1.nupkg`
with all three native libraries staged into it, and the browser demo built.
Until this run every Linux and macOS leg in `release.yml` was configuration
nobody had watched, and "one tag produces all four" was a claim rather than a
measurement. **Nothing was published, and nothing has been.**

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

using var signatures = document.ReadSignatures();
using var anchors = new TrustAnchors();      // empty: the host trusts nothing
anchors.Add(File.ReadAllBytes("root.der"));  // or says what it does
using var verdicts = document.VerifySignatures(anchors);
for (uint i = 0; i < signatures.Count; i++)
{
    // Four answers, never one boolean.
    _ = (signatures.CoverageOf(i), verdicts.DocumentDigestOf(i),
         verdicts.SignatureCheckOf(i), verdicts.ChainOf(i));
}
```

```c
TpdfDocument *doc = NULL;
if (tpdf_document_open(bytes, len, &doc) == 0 /* TpdfStatus::Ok */) {
    TpdfBitmap *bm = NULL;
    tpdf_page_render(doc, 0, 2.0, 2 /* TpdfPixelFormat::Rgb8 */, &bm);
    /* tpdf_bitmap_width(bm), tpdf_bitmap_stride(bm), tpdf_bitmap_data(bm, ...) */
    tpdf_bitmap_free(bm);

    TpdfSignatures *sigs = NULL;
    TpdfTrustAnchors *anchors = tpdf_trust_anchors_new();
    TpdfVerdicts *verdicts = NULL;
    tpdf_document_signatures(doc, &sigs);
    tpdf_document_verify_signatures(doc, anchors, 0 /* judge validity */, 0, &verdicts);
    for (uint32_t i = 0; i < tpdf_signatures_count(sigs); i++) {
        char *reason = NULL;             /* NULL on Ok means /Reason is absent */
        tpdf_signature_reason(sigs, i, &reason);
        tpdf_string_free(reason);
    }
    tpdf_verdicts_free(verdicts);
    tpdf_trust_anchors_free(anchors);
    tpdf_signatures_free(sigs);

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
| Editing, forms, creation, saving | not present on any binding — the surface is open, page count, encryption/auth, permissions, page size, text, render, `set_fonts`, and (C ABI and .NET only) signature reading | the write surface has not been projected; the facade shape is already the design | [ROADMAP.md](../ROADMAP.md) (design/bindings-write.md) |
| Signing: `save_signed`, `Signer` | no `tpdf_*` entry point takes a callback | a signer is a host callback, and callbacks across the C ABI are an explicit non-goal of the write design, which owns them | [ROADMAP.md](../ROADMAP.md) (design/bindings-write.md) |
| The payloads inside a signature enum — which revision, which defect, whose certificate, how many bits | the enum arm crosses, the payload does not | a C enum has no payload, and a struct invented here to carry one would be this crate spelling something the facade already spells (ruling 11) | [signatures](../design/signatures.md) |
| A signature's `/Contents` blob, `/M`, `/ContactInfo`, `/Filter`, its lenient-read warnings, and `Signature::modifications` | not projected | owed rather than refused: each is a shape of its own — raw bytes, a date, a list of changed objects — rather than another string or enum, and none is named by the milestone | [signatures](../design/signatures.md) |
| Signatures in Python and JavaScript | not projected | those bindings sit on the facade directly rather than on the C ABI, so each is its own transcription and neither has been written | [ROADMAP.md](../ROADMAP.md) |
| Outline, links, metadata, attachments, XMP, warnings | not projected | the read surface beyond rendering, text and signatures is owed | [ROADMAP.md](../ROADMAP.md) |
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
  refusals).
- The signature surface is pinned by an **equality with the facade**, which
  is what makes it a projection rather than a second implementation: a
  fixture signed twice through `DocumentEditor::save_signed` with a stub
  signer — in the test, so it runs without the fetched corpus — is read
  through the C ABI and through `Document::signatures` /
  `verify_signatures`, and every field name, sub-filter, `/Reason`,
  `/Location`, `/Name`, coverage, span, CMS state, digest, signature check,
  chain, signer subject/issuer, validity window and weakness must agree.
  Signed twice because the second signature is what leaves the first
  covering only a revision, which is the one shape that gives a verdict a
  weakness to carry. Alongside it: a null document, a null handle on every
  accessor, an index past the last signature (`NoSuchSignature`, and a span
  or weakness index past the end as `BadArgument`, because the two are
  different mistakes), an unsigned document answering "none" rather than
  failing, an anchor that is not a certificate refused and not kept, the
  instant ignored unless the flag says otherwise, and signatures outliving
  the document they came from. The crate type-checks in CI on every commit (the bindings
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
