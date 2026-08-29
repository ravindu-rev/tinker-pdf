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
`UnsupportedHandler`, `NoSuchSignature`, `NoSuchField`, `ValueRefused`,
`FieldUnreadable`, `SpentHandle`, `EditRefused`) and
`tpdf_last_error_message` carries the detail. Those numbers *are* the ABI —
a C caller compares them against literals and the .NET binding against an
`int` — so 0–7 are frozen, `NoSuchSignature` was **appended** at 8 rather
than inserted, and the write surface's five were appended at 9–13. Two unit
tests pin them: one names all fourteen individually, and one holds the list
and its length, so a variant added without a line is caught by the count
rather than by somebody remembering. A third pins every discriminant of the
six signature enums and the three write ones, because those are transcribed
by hand into `bindings/dotnet/TinkerPdf.cs`.

The threading rule differs on the two halves, and it is not a caveat but a
consequence. A `TpdfDocument` may be used from any thread because every read
borrows an immutable, shared `Document`. A `TpdfEditor`, `TpdfBuilder` or
`TpdfPageBuilder` is **mutable state**: the calls take `&mut`, so two threads
in one handle is the same data race it would be in Rust, and no C ABI can
stop it. One handle per thread, or the caller's own lock; freeing stays safe
from any thread.

**One hundred and three functions**, of which fifty-five are the write
surface below. Eighteen open and render: `tpdf_version`,
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

**The write surface: fifty-five functions, and the shape they had to be
given.** The facade has exported `DocumentEditor` and `DocumentBuilder` since
gap 26, so what stood between the read surface and this one was never
capability. It was *shape*: `DocumentEditor::transaction(|tx| ..)` and
`DocumentBuilder::add_page(w, h, |page| ..)` take closures, and a closure does
not cross this boundary. Ruling 11 answers that — the facade grows the
closure-free equivalent first, and then the C ABI is a mechanical wrapping of
a Rust API that already exists. So the facade gained
`DocumentEditor::checkpoint` / `restore` and `DocumentBuilder::begin_page` /
`push_page`, with both closure APIs reimplemented as callers of them
([design/bindings-write.md](../design/bindings-write.md)).

*A checkpoint is a value, not an open transaction.* That is what makes it safe
to hand across an ABI where a `begin`/`commit`/`rollback` triple would not be:
taking one changes nothing, freeing one commits nothing because nothing was
pending, and `tpdf_editor_restore` is **idempotent** — restoring twice is
restoring once, which is what a host language's `finally` running after its own
`catch` needs. The checkpoint is borrowed rather than consumed, so one can
undo several attempts: the retry loop a closure cannot express.

*Five handles.* `TpdfEditor` (`tpdf_document_editor` / `_free`, and
`tpdf_editor_is_dirty` / `_page_count` / `_delete_page` / `_move_page` /
`_rotate_page` / `_insert_page` / `_set_crop_box` / `_append_content` /
`_field_count` / `_field_name` / `_field_value` / `_fill_field` /
`_set_checkbox` / `_select_radio` / `_checkpoint` / `_restore` / `_save`);
`TpdfCheckpoint` (`_free`); `TpdfBuffer` (`tpdf_buffer_data` / `_len` /
`_free`, borrowing until freed on the `TpdfBitmap` pattern exactly);
`TpdfBuilder` (`tpdf_builder_new` / `_free` / `_add_base_font` /
`_add_embedded_font` / `_set_subset_fonts` / `_add_image` / `_set_info` /
`_begin_page` / `_push_page` / `_set_outline` / `_finish`); and
`TpdfPageBuilder` (`tpdf_page_builder_text` / `_fill_rect` / `_image` /
`_set_fill_rgb` / `_set_stroke_rgb` / `_set_crop_box` / `_raw` / `_link` /
`_free`). An editor is independent of the document it came from —
`Document::editor()` clones the shared `Arc<CosDocument>` — so freeing the
document first is legal and the .NET `SafeHandle`s need no parent-child
keep-alive; a test asserts exactly that rather than leaving it inferred.

*Consuming calls are a double-free factory,* and that is the design problem
this surface actually had. `DocumentBuilder::finish(self)` and `push_page` and
the outline's `add_child` consume in Rust; a C caller has a pointer, and a
pointer that has been "consumed" is one the caller will still free and may
still use. So a consumable handle boxes an `Option`: the consuming call takes
the value and **records which call took it**, the handle stays live and stays
the caller's to free, and any later call on it is `SpentHandle` with a message
naming the call that spent it. Free therefore stays symmetric with allocation
and stays null-tolerant, exactly as everywhere else on this boundary. A page
begun and never pushed is simply freed, and the document is byte-for-byte what
it would have been — asserted, because that is the property that makes
abandoning a handle safe rather than merely non-fatal.

*`SkippedWidget` is a fourth outcome and does not flatten into failure.* A
fill has three answers, not two: a non-`Ok` status means **nothing was
written** (`NoSuchField`, `ValueRefused`, `FieldUnreadable` — 12.7.4.3, refusal
over truncation, because truncating hides a data error inside a file that then
looks correctly filled); `Ok` with an empty `TpdfFillReport` means the value
was written and every widget drawn; `Ok` with a non-empty one means the value
was written and those widgets were left showing whatever they showed before,
because 12.5.2's required `/Rect` is missing from them. Ruling 2 degrades
rather than failing; ruling 10 makes the degradation name its object, so
`tpdf_fill_report_widget` hands back the `ObjRef` as a number and generation
and `tpdf_fill_report_defect` the `TpdfWidgetDefect` — not only
`tpdf_fill_report_message`'s sentence about them.

*Two flat `#[repr(C)]` option structs.* `TpdfWriteOptions` maps `WriteOptions`
field for field, with the booleans and the version pair widened to integers so
a hand-written P/Invoke has no packing to guess at, and
`tpdf_write_options_init` fills it with **the facade's own defaults** — because
a C caller who guesses them writes a different file than a Rust caller with the
same intent, which is the whole failure the parity suite exists to catch.
`TpdfEncryption` hangs off it or is null. **No binding invents entropy**: the
48 bytes are the caller's, and a length that is not 48 is `BadArgument` rather
than a buffer read to 48 out of whatever followed it in the caller's address
space. `TpdfDestination` carries all eight of `DestKind`'s arms rather than a
convenient subset, and spells `Option<f64>` as **NaN meaning null** (12.3.2.2's
"retain the current value") — unambiguous because the writer refuses a
non-finite number as a coordinate anywhere else, and cheaper than a presence
mask that can fall out of step with the values it describes.
`tpdf_destination_init_fit` exists because a *zeroed* `TpdfDestination` is
`/XYZ 0 0 0`, which is a different destination that merely looks like a
default.

*`EditRefused` is one status and not four,* on purpose. `delete_page`,
`move_page`, `rotate_page`, `insert_page`, `set_crop_box`, `set_checkbox`,
`select_radio` and `append_content` answer `bool` or `Option` on the facade and
name no reason: an index that does not exist and a page object that is not a
dictionary are the same `false` there. A C ABI that split them would be
guessing, and a caller would believe the guess — the same argument that keeps
the signature enums' payloads from crossing. What crosses instead is
provenance: the message names the call and the argument it refused
(`delete_page refused: index 99`), and `tpdf_editor_page_count` /
`tpdf_editor_field_count` let a caller tell the bounds case apart *before* the
call rather than after.

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

```c
/* Fill a form and save incrementally. */
TpdfEditor *ed = NULL;
tpdf_document_editor(doc, &ed);
tpdf_document_free(doc);              /* legal: the editor holds its own */

TpdfFillReport *report = NULL;
if (tpdf_editor_fill_field(ed, "name", "Ada Lovelace", &report) == 0) {
    /* Ok, and the report may still be non-empty: value written, some
       widget not drawable. That is a fourth outcome, not a failure. */
    for (uint32_t i = 0; i < tpdf_fill_report_count(report); i++) {
        uint32_t num = 0; uint16_t gen = 0;
        tpdf_fill_report_widget(report, i, &num, &gen);
    }
    tpdf_fill_report_free(report);
}

TpdfWriteOptions options;
tpdf_write_options_init(&options);    /* the facade's defaults, not zeros */
options.mode = 1;                     /* TpdfWriteMode::Incremental */

TpdfBuffer *out = NULL;
tpdf_editor_save(ed, &options, &out);
/* tpdf_buffer_data(out, &len) borrows until tpdf_buffer_free */
tpdf_buffer_free(out);
tpdf_editor_free(ed);

/* Build a document. begin_page/push_page, because closures do not cross. */
TpdfBuilder *b = NULL;
tpdf_builder_new(&b);
tpdf_builder_add_base_font(b, (const uint8_t *)"F1", 2,
                           (const uint8_t *)"Helvetica", 9);

TpdfPageBuilder *page = NULL;
tpdf_builder_begin_page(b, 200.0, 200.0, &page);
tpdf_page_builder_text(page, (const uint8_t *)"F1", 2, 14.0, 20.0, 170.0,
                       "Page one");
tpdf_builder_push_page(b, page);      /* consumes the drawing */
tpdf_page_builder_free(page);         /* the handle is still yours */

TpdfBuffer *pdf = NULL;
tpdf_builder_finish(b, &pdf);         /* consumes the document */
/* a second finish here is TpdfStatus::SpentHandle (12), never a double free */
tpdf_buffer_free(pdf);
tpdf_builder_free(b);
```

(The crate ships no generated header; the `#[repr(C)]` enums and structs —
`TpdfStatus`, `TpdfAuthLevel`, `TpdfPixelFormat`, the six signature enums,
`TpdfWidgetDefect`, `TpdfWriteMode`, `TpdfDestKind`, `TpdfTargetKind`,
`TpdfImageKind`, `TpdfWriteOptions`, `TpdfEncryption`, `TpdfDestination`,
`TpdfTarget`, `TpdfImage` — and the `extern "C"` signatures in
`crates/tinker-pdf-ffi/src/lib.rs` are the contract, and the .NET binding's
P/Invoke declarations are a worked transcription of them.)

Each binding's README ([js](../../bindings/js/README.md),
[python](../../bindings/python/README.md),
[dotnet](../../bindings/dotnet/README.md)) carries its build, smoke-test and
packaging commands.

## Refused by name

| What | How it shows | Why | See |
| --- | --- | --- | --- |
| `ImageData::Compressed` | `TpdfImageKind` has `Jpeg`, `Rgb8` and `Gray8` and no fourth arm | it carries a `CompressedImage` whose colour space holds a palette slice and whose filter holds its own parameters, so projecting it is a sub-surface rather than a struct. It exists for the CBZ synthesiser, which must not decode 200 pages at open — an engine-internal path with no host at the other end. A host holding already-compressed bytes has `Jpeg`, which is the same idea for the one codec hosts actually hold bytes in | [creation](creation.md) |
| `PageBuilder::tagged` | not projected; a page is drawn untagged through the C ABI | it nests *within* one page and takes a closure whose scope is the structure element's extent, so the closure-free spelling is an `open_tag`/`close_tag` pair on the page handle — a separate question with a separate answer, and neither parity script tags anything. `begin_page`/`push_page` compose with it, which is asserted, so nothing here has to be undone to add it | [tagged-pdf](tagged-pdf.md) |
| The rest of `PageBuilder` — `encoded_text`, `glyphs`, `form`, `shading`, `set_fill_pattern`, `set_stroke_pattern`, `set_ext_gstate`, `set_bleed_box` — and `DocumentBuilder`'s `add_named_font`, `add_cid_font`, `glyph_run`, `add_ext_gstate`, `add_form`, `add_shading`, `add_tiling_pattern`, `clear_image_resources` | not projected | owed rather than refused: each takes an argument type of its own (`ExtGState`, `Shading`, `TilingPattern`, `Glyph`) that would need its own flat `#[repr(C)]` spelling, and none is named by the write milestones. `tpdf_page_builder_raw` is the escape hatch that keeps them reachable in the meantime | [ROADMAP.md](../ROADMAP.md) |
| `DocumentEditor`'s `import_page`, `keep_pages`, `flatten_annotations`, `add_annotation`, `reset_form`, `recalculate`, `set_field_values`, `set_calculated_values` | not projected | the same: owed, each with a shape of its own — a second document, a slice of indices, a `Dict`, a `Recalculation` — and none named by the milestones | [ROADMAP.md](../ROADMAP.md) |
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
- **The write surface is pinned by the same equality with the facade** that
  the signature surface is, and for the same reason: it is what makes it a
  projection rather than a second writer. The fill-and-save script and the
  build-a-document script are each written twice, once through the C ABI and
  once against the facade in Rust, and the two must produce **the same
  bytes** — not a valid document, not a similar one. Around them: the
  incremental save's original-bytes prefix (7.5.6) asserted on the C ABI's own
  output and the result re-opened through the strict validator (ruling 13);
  the fill report's widget `ObjRef` and defect, with the undamaged control
  field proving an empty report is reachable; each `FillError` variant
  arriving as its own status; a checkpoint round trip with three redundant
  restores to show idempotence; an editor outliving its document; a refusal
  naming the call and argument; `tpdf_write_options_init` equalling
  `WriteOptions::default()` *and* being what the save uses; encryption
  byte-equal with fixed entropy and refused with 47 bytes; all eight
  destination kinds with NaN as null; a double `finish` and a double
  `push_page` returning `SpentHandle` and naming the call that spent the
  handle; an abandoned page leaving the document byte-identical; and a null on
  every one of the fifty-five entry points with every new `tpdf_*_free`
  accepting null.
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
