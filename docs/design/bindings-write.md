# Bindings write surface

The binding surfaces — the C ABI, Python, JS/wasm and .NET, with Go, Ruby and
Java over the C ABI — project `DocumentEditor` and `DocumentBuilder` the way
they project `Document`: open a form, fill it, save it incrementally; build a
document from pages, fonts and images; and prove every surface did the same
thing by hashing the bytes ([features/bindings.md](../features/bindings.md)).
The facade exports `DocumentBuilder`, `DocumentEditor`, `PageBuilder`,
`WriteOptions`, `WriteMode` and `Encryption` (`crates/tinker-pdf/src/lib.rs`),
and `Document::editor()` hands an editor over the shared `Arc<CosDocument>`.
What a binding has to cross is not capability but shape:
`DocumentEditor::transaction(|tx| ..)` and
`DocumentBuilder::add_page(w, h, |page| ..)` take closures, and closures do not
cross an FFI boundary. Ruling 11 ([rulings.md](../rulings.md)) says what to do
about that: the facade grows the closure-free equivalents first, and every
binding stays a 1:1 projection with no logic of its own.

## Scope

- Facade primitives in `crates/tinker-pdf` (and `tinker-pdf-cos` beneath it):
  closure-free primitives that the closure APIs are sugar over —
  an editor checkpoint/restore pair, and an owned-`PageBuilder`
  begin/push pair — with the transactional semantics of
  `DocumentEditor::transaction` preserved and stated.
- The C ABI in `crates/tinker-pdf-ffi`: editor, builder, page-builder,
  checkpoint and byte-buffer handles under the crate's ownership rule
  ("the engine allocates and the matching `tpdf_*_free` releases"), append-only
  `TpdfStatus` growth, and typed refusals crossing the boundary intact
  (rulings 2 and 10).
- Idiomatic wrappers in `bindings/python` (PyO3, facade-direct),
  `bindings/js` (wasm-bindgen, facade-direct) and `bindings/dotnet`
  (`SafeHandle` over the C ABI), each 1:1 per ruling 11.
- Write-side parity: one fill-and-save script and one build-a-document script
  driven from every surface, asserting byte-identical output by SHA-256,
  run from the installed artefacts (`write_parity.py`, `write_parity.mjs`,
  `bindings/dotnet/tests/Smoke/Program.cs`) and checked by `cargo xtask`.
- Documentation: `docs/features/bindings.md` describes the write surface
  beside the committed C header, and the three package READMEs follow.

## Non-goals

- No new facade capability. Everything projected here exists on
  `DocumentEditor` and `DocumentBuilder`; APIs the facade lacks
  (e.g. content-editing beyond `append_content`) are out of scope.
  Content editing is [ROADMAP](../ROADMAP.md) rows ED-01 and ED-02, and
  reaches the facade first.
- No callbacks into host code. `FontProvider` stays the byte-blob
  `SimpleFontProvider` projection `tpdf_document_set_fonts` is; no host
  closure runs inside the engine on this surface, so no unwind crosses the
  boundary. A signing callback across the C ABI is [ROADMAP](../ROADMAP.md)
  row SG-09, with script host requests (FJ-17) and warning and progress
  callbacks (SU-21) beside it; how each crosses is H-28.
- No streaming or file-handle I/O. Bytes in, bytes out
  (ruling 8 discipline at the boundary).
- No new packages or registries. The wheel/npm/NuGet pipeline
  (`cargo xtask release`, `nuget-stage`, `release.yml`) carries the write
  surface, and its smoke jobs run the write legs.
- No entropy defaults. `Encryption::entropy` is 48 caller-supplied bytes by
  design (`tinker-pdf-cos/src/write.rs`); no binding substitutes its own
  randomness, because a binding that invents a default violates ruling 11 and
  hides the one input that makes encrypted output non-reproducible.

## Design

**The facade grows first: two closure-free primitives.** The closure APIs are
kept — they are the right Rust — and implemented as sugar over primitives
that cross FFI:

- *Editor checkpoints.* `DocumentEditor::checkpoint() -> EditCheckpoint` and
  `DocumentEditor::restore(&mut self, &EditCheckpoint)`
  (`tinker-pdf-cos/src/edit/mod.rs`) make the editor's own state — its
  overlay, deletions, object counter and page order among it — a public
  opaque value; `transaction()` is checkpoint → body → restore-on-`Err`. The
  doc comment on `transaction` rejects a
  begin/commit/rollback triple because a triple leaves an editor in a state a
  later reader cannot classify; checkpoints preserve exactly that property
  across FFI. There is no "open" state: a checkpoint is a value, restoring is
  idempotent, dropping one commits nothing because nothing was pending. The
  transactional contract — `Err` restores objects written, objects deleted,
  page order and the object-number counter — is inherited, not re-proven,
  because both forms run the same two functions.
- *Owned pages.* `DocumentBuilder::begin_page(w, h) -> PageBuilder` and
  `DocumentBuilder::push_page(&mut self, PageBuilder)`
  (`tinker-pdf-cos/src/build.rs`) split `add_page(w, h, draw)`, which is
  begin → draw → push. `begin_page` takes the snapshot of the builder's
  resource set the page draws with, so a font registered after `begin_page`
  is invisible to that page — the same timing the closure form imposes.
  `PageBuilder` has no public constructor; a page is born from a builder or
  not at all.

**The C ABI in `tinker-pdf-ffi`.** Five opaque handle types beside
`TpdfDocument` and `TpdfBitmap`: `TpdfEditor`, `TpdfBuilder`,
`TpdfPageBuilder`, `TpdfCheckpoint`, `TpdfBuffer`, each with its `tpdf_*_free`
accepting null, per the crate's ownership rule stated once at the top of
`src/lib.rs`.

- *Lifetimes.* `tpdf_document_editor(doc, out)` projects
  `Document::editor()`; because the editor holds its own `Arc<CosDocument>`,
  the editor handle is independent of the document handle — freeing the
  document first is legal, and the .NET `SafeHandle`s need no parent-child
  keep-alive.
- *Consuming calls.* `DocumentBuilder::finish(self)` and `push_page`
  consume in Rust, and a consuming FFI call is a double-free factory. The
  handle boxes an `Option<T>`: `tpdf_builder_finish` and
  `tpdf_builder_push_page` `take()` the value, later calls on a spent handle
  return `TpdfStatus::SpentHandle` with `tpdf_last_error_message` saying
  which call spent it, and `tpdf_*_free` stays required and safe. Free remains
  symmetric with allocation everywhere.
- *Bytes out.* `tpdf_editor_save(editor, options, out)` and
  `tpdf_builder_finish(builder, out)` yield a `TpdfBuffer` —
  `tpdf_buffer_data`/`tpdf_buffer_len` borrow until `tpdf_buffer_free`,
  exactly the `TpdfBitmap` pattern. Save options cross as a flat
  `#[repr(C)]` struct, `TpdfWriteOptions`: mode (rewrite/incremental, 7.5.6),
  linearize (Annex F), the version, object streams, compression, garbage
  collection, and an optional encryption block (user/owner passwords,
  permission bits, the 48 entropy bytes), mapping `WriteOptions` field for
  field but `deduplicate_streams`, which crosses at its default, off.
- *Errors.* `TpdfStatus` grows append-only: a status keeps its number.
  `FillError`'s variants (`NoSuchField`, `ValueRefused`,
  `FieldUnreadable` — 12.7.4.3, refusal over truncation) map to distinct
  statuses; editor methods that return `bool`/`Option` (`delete_page`,
  `rotate_page`, `insert_page`) map false/None to `EditRefused`, with a
  message naming the call and the argument it refused.
  `SkippedWidget` is the fourth outcome — written but not wholly drawable —
  and is not flattened into failure: `tpdf_editor_fill_field` returns
  `Ok` plus a `TpdfFillReport` handle the caller iterates
  (`tpdf_fill_report_count`, `tpdf_fill_report_message(i)`,
  `tpdf_fill_report_widget(i)`, `tpdf_fill_report_defect(i)`), preserving
  ruling 10's provenance — each entry names its widget `ObjRef` and
  `WidgetDefect`; the field is the call's own argument.

**Wrappers stay 1:1.** Python and JS link the facade directly (PyO3,
wasm-bindgen), so they project `checkpoint`/`restore`, `begin_page`/
`push_page`, `fill_field`, `save`, `finish` as methods on `PyEditor`/
`PyBuilder` and `PdfEditor`/`PdfBuilder` types. Each may offer its language's
natural sugar — a Python context manager, a .NET callback-taking
`Transaction` — **implemented as nothing but checkpoint, host-language control
flow, restore**: the semantics stays the facade's, per ruling 11. JavaScript
offers none: an exported wasm-bindgen method borrows its `this` for the whole
call, so a callback that touched the same editor would fail, and the three
lines are the caller's ([bindings/js/README.md](../../bindings/js/README.md)).
An exception thrown by host code inside that sugar takes the restore path,
mirroring `Err`; no host code runs inside the engine, so no unwind crosses a
language boundary. .NET consumes the C ABI with `EditorHandle`,
`BuilderHandle`, `PageBuilderHandle`, `CheckpointHandle` and `BufferHandle`
following the `DocumentHandle`/`BitmapHandle` `SafeHandle` pattern in
`bindings/dotnet/TinkerPdf.cs`, and maps non-`Ok` statuses to `PdfException`
carrying the status and `tpdf_last_error_message`.

**Parity is byte identity.** Two scripts, fixed inputs: *fill-and-save* opens
a form fixture, fills named fields, saves incrementally (7.5.6 — the original
bytes must survive as a prefix, the property signatures depend on per 12.8.1).
The fixture is `testdata/form-fields.pdf`: the hand-written `/AcroForm` shapes
`edit/tests.rs`'s own inline test fixtures use, with one widget
deliberately lacking `/Rect` so the skipped-widget report leg is exercised,
not just possible. `testdata/form-fields.py` writes it, every object spelled
out by hand (`python testdata/form-fields.py testdata/form-fields.pdf`), and
`crates/tinker-pdf-cos/tests/form_fixture.rs` holds the committed bytes
([testdata/README.md](../../testdata/README.md)). *Build-a-document*
registers a base font and an image, draws two pages, sets info and outline,
finishes.
Both run with pinned `WriteOptions`, and an encrypted save is given fixed
entropy — the one input that would otherwise vary — so ruling 4's determinism
contract makes byte-identical output the *expected* result, not a lucky one.
The recorded hashes live in `xtask/src/parity.rs`; each surface's write leg
prints `WROTE sha256=<hex>`, and `cargo xtask bindings-parity` compares every
surface's lines against the recorded hash and exits nonzero on any mismatch or
any missing line. The saved bytes are additionally re-opened
through this engine's strict structural validator, which is what keeps
byte-identical outputs from being identically wrong — under ruling 13 that
check is first-party, so it cannot be absent and cannot be skipped.

**Packaging.** No new artifacts: the wheel, npm tarball and NuGet package
carry the write symbols. `cargo xtask release` (dry run by default),
`nuget-stage` and `release.yml`'s smoke gates include the write legs, so a
package that ships a read-only `cdylib` fails its own release pipeline.

## Milestones

1. Facade primitives: `EditCheckpoint`/`restore` and `begin_page`/`push_page`,
   the closure APIs as sugar over them, and the `testdata/form-fields.pdf`
   fixture.
2. The C ABI editor surface: `TpdfEditor`, `TpdfCheckpoint`, `TpdfBuffer`,
   `TpdfFillReport`, page operations, fill and save, with `TpdfStatus`
   appended.
3. The C ABI builder surface: `TpdfBuilder` and `TpdfPageBuilder`, push and
   finish over `Option<T>` boxing.
4. The Python and JS wrappers: `PyEditor`/`PyBuilder` and
   `PdfEditor`/`PdfBuilder`, with Python's transaction sugar over the two
   primitives.
5. The .NET wrapper: the five `SafeHandle`s, `Editor`/`Builder` classes and
   the status-to-`PdfException` mapping.
6. Parity, validation and CI: the recorded write hashes,
   `cargo xtask bindings-parity`, the strict-validator leg and the release
   smoke gates.

## Dependencies

- The packaging pipeline (`xtask release`, `nuget-stage`,
  `release.yml`) and the smoke tests it drives.
- Ruling 4's determinism contract
  ([features/determinism.md](../features/determinism.md)), which makes
  byte-identical output the expected result.
- The strict structural validator ([verification.md](../verification.md))
  for the validation leg.
- [docs/features/bindings.md](../features/bindings.md) (referenced by
  `tinker-pdf-ffi/src/lib.rs`) documents the surface; the projections owed are
  listed in [ROADMAP.md](../ROADMAP.md).

## Risks

| Risk | Mitigation |
| --- | --- |
| Consuming Rust APIs (`finish`, `push_page`) become FFI double-frees | Handles box `Option<T>`; consuming calls `take()`, spent handles refuse with `SpentHandle`, free stays symmetric and null-tolerant — asserted in the C ABI's tests |
| Checkpoint pair reintroduces the silent-misuse `transaction()` was designed against | No open state exists by construction: checkpoints are values, restore is idempotent, drop is inert; the Python and .NET wrappers ship the closure sugar so callers rarely touch the pair raw |
| `TpdfStatus` growth breaks the C ABI | Append-only discriminants; a `tinker-pdf-ffi` test asserts every variant's number |
| Byte-identical parity is brittle against legitimate writer changes | Hashes live in one place (`xtask/src/parity.rs`) and every surface is compared to *it*, so a writer change is one recorded update, not a flaky suite per surface; the strict validator is what keeps "identical" from meaning "identically wrong" |
| Encrypted output non-reproducible across surfaces | Entropy is caller-supplied by design; tests pass fixed bytes, and no binding is permitted a randomness default (ruling 11) |
| Skipped widgets flattened into success or failure across FFI | `TpdfFillReport` carries each `SkippedWidget` with its widget `ObjRef` and `WidgetDefect` (ruling 10); smoke tests assert the report crosses non-empty on the `/Rect`-less widget `testdata/form-fields.pdf` carries for exactly this |
| wasm memory growth invalidates borrowed views mid-edit | Write APIs return copies (`TpdfBuffer` on the C ABI, owned `Vec<u8>`/`bytes` in wasm and Python); the only aliasing view remains the read side's `viewUnsafeUntilNextAllocation`, whose detachment `node_smoke.mjs` demonstrates |
