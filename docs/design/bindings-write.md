# Bindings write surface

When this is done, the four binding surfaces — the C ABI, Python, JS/wasm and
.NET — will project `DocumentEditor` and `DocumentBuilder` the way they already
project `Document`: open a form, fill it, save it incrementally; build a
document from pages, fonts and images; and prove all four did the same thing by
hashing the bytes, the write-side analogue of the read-side dry run where all
four surfaces produced the same 1 190 inked pixels
(gap 26, [features/bindings.md](../features/bindings.md)). Today every binding is read-only — open,
page count, authenticate, permissions, page size, text, render, `set_fonts` —
while the facade already exports `DocumentBuilder`, `DocumentEditor`,
`PageBuilder`, `WriteOptions`, `WriteMode` and `Encryption`
(`crates/tinker-pdf/src/lib.rs`) and `Document::editor()` already hands an
editor over the shared `Arc<CosDocument>`. The distance is not capability but
shape: `DocumentEditor::transaction(|tx| ..)` and
`DocumentBuilder::add_page(w, h, |page| ..)` take closures, and closures do not
cross an FFI boundary. Ruling 11 ([rulings.md](../rulings.md)) says what to do
about that: the facade grows the closure-free equivalents first, and every
binding stays a 1:1 projection with no logic of its own.

## Scope

- Facade growth in `crates/tinker-pdf` (and `tinker-pdf-cos` beneath it):
  closure-free primitives that the existing closure APIs become sugar over —
  an editor checkpoint/restore pair, and an owned-`PageBuilder`
  begin/push pair — with the transactional semantics of
  `DocumentEditor::transaction` preserved and stated.
- C ABI growth in `crates/tinker-pdf-ffi`: editor, builder, page-builder,
  checkpoint and byte-buffer handles under the crate's existing ownership rule
  ("the engine allocates and the matching `tpdf_*_free` releases"), append-only
  `TpdfStatus` growth, and typed refusals crossing the boundary intact
  (rulings 2 and 10).
- Idiomatic wrappers in `bindings/python` (PyO3, facade-direct),
  `bindings/js` (wasm-bindgen, facade-direct) and `bindings/dotnet`
  (`SafeHandle` over the C ABI), each 1:1 per ruling 11.
- Write-side parity: one fill-and-save script and one build-a-document script
  driven from all four surfaces, asserting byte-identical output by SHA-256,
  wired into the existing smoke tests (`wheel_smoke.py`, `node_smoke.mjs`,
  `bindings/dotnet/tests/Smoke/Program.cs`) and checked by `cargo xtask`.
- Documentation: `docs/features/bindings.md` grows the write surface; its
  `extern "C"` contract listing (the crate ships no generated header) and
  the three package READMEs follow.

## Non-goals

- No new facade capability. Everything projected here exists on
  `DocumentEditor` and `DocumentBuilder` today; APIs the facade lacks
  (e.g. content-editing beyond `append_content`) are out of scope.
- No callbacks into host code. `FontProvider` stays the byte-blob
  `SimpleFontProvider` projection `tpdf_document_set_fonts` already is; no
  host closure ever runs inside the engine, so no unwind crosses the boundary.
- No streaming or file-handle I/O. Bytes in, bytes out, as everywhere
  (ruling 8 discipline at the boundary).
- No new packages or registries. The wheel/npm/NuGet pipeline from gap 26
  (`cargo xtask release`, `nuget-stage`, `release.yml`) carries the new
  surface; only its smoke tests grow.
- No entropy defaults. `Encryption::entropy` is 48 caller-supplied bytes by
  design (`tinker-pdf-cos/src/write.rs`); no binding substitutes its own
  randomness, because a binding that invents a default violates ruling 11 and
  hides the one input that makes encrypted output non-reproducible.

## Design

**The facade grows first: two closure-free primitives.** The closure APIs are
kept — they are the right Rust — and reimplemented as sugar over primitives
that cross FFI:

- *Editor checkpoints.* `DocumentEditor::checkpoint() -> EditCheckpoint` and
  `DocumentEditor::restore(&mut self, &EditCheckpoint)` make the private
  `Snapshot` in `tinker-pdf-cos/src/edit.rs` (overlay, deletions, object
  counter, page order) a public opaque value; `transaction()` becomes
  checkpoint → body → restore-on-`Err`, which is what it already is
  internally. The doc comment on `transaction` rejects a
  begin/commit/rollback triple because a triple leaves an editor in a state a
  later reader cannot classify; checkpoints preserve exactly that property
  across FFI. There is no "open" state: a checkpoint is a value, restoring is
  idempotent, dropping one commits nothing because nothing was pending. The
  transactional contract — `Err` restores objects written, objects deleted,
  page order and the object-number counter — is inherited, not re-proven,
  because both forms run the same two functions.
- *Owned pages.* `DocumentBuilder::begin_page(w, h) -> PageBuilder` and
  `DocumentBuilder::push_page(&mut self, PageBuilder)` split
  `add_page(w, h, draw)` at the line it already contains: `add_page`
  constructs a `PageBuilder` carrying a snapshot of the builder's resource
  set, runs the closure, pushes. `begin_page` snapshots at the same moment —
  so a font registered after `begin_page` is invisible to that page, the same
  timing the closure form imposes today — and `add_page` becomes
  begin → draw → push. `PageBuilder` gains no public constructor; a page is
  born from a builder or not at all.

**C ABI growth in `tinker-pdf-ffi`.** Five new opaque handle types beside
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
  return a new `TpdfStatus::SpentHandle` with `tpdf_last_error_message` saying
  which call spent it, and `tpdf_*_free` stays required and safe. Free remains
  symmetric with allocation everywhere.
- *Bytes out.* `tpdf_editor_save(editor, options, out)` and
  `tpdf_builder_finish(builder, out)` yield a `TpdfBuffer` —
  `tpdf_buffer_data`/`tpdf_buffer_len` borrow until `tpdf_buffer_free`,
  exactly the `TpdfBitmap` pattern. Save options cross as a flat
  `#[repr(C)]` struct: mode (rewrite/incremental, 7.5.6), linearize (Annex F),
  object streams, and an optional encryption block (user/owner passwords,
  permission bits, the 48 entropy bytes) mapping `WriteOptions` field-for-field.
- *Errors.* `TpdfStatus` grows append-only (existing discriminants 0–7 are
  frozen ABI). `FillError`'s variants (`NoSuchField`, `ValueRefused`,
  `FieldUnreadable` — 12.7.4.3, refusal over truncation) map to distinct
  statuses; editor methods that return `bool`/`Option` today (`delete_page`,
  `rotate_page`, `insert_page`) map false/None to statuses naming the reason.
  `SkippedWidget` is the fourth outcome — written but not wholly drawable —
  and must not be flattened into failure: `tpdf_editor_fill_field` returns
  `Ok` plus a `TpdfFillReport` handle the caller iterates
  (`tpdf_fill_report_count`, `tpdf_fill_report_message(i)`), preserving
  ruling 10's provenance — each entry renders a `SkippedWidget`, which names
  its widget `ObjRef` and `WidgetDefect`; the field is the call's own
  argument.

**Wrappers stay 1:1.** Python and JS link the facade directly (PyO3,
wasm-bindgen), so they project `checkpoint`/`restore`, `begin_page`/
`push_page`, `fill_field`, `save`, `finish` as methods on new `PyEditor`/
`PyBuilder` and `PdfEditor`/`PdfBuilder` types. Each may offer its language's
natural sugar — a Python context manager, a JS/.NET callback-taking
`transaction` — **implemented as nothing but checkpoint, host-language control
flow, restore**: the semantics stays the facade's, per ruling 11. An exception
thrown by host code inside that sugar takes the restore path, mirroring `Err`;
no host code ever runs inside the engine, so no unwind crosses a language
boundary. .NET consumes the C ABI with `EditorHandle`, `BuilderHandle`,
`PageBuilderHandle`, `CheckpointHandle` and `BufferHandle` following the
existing `DocumentHandle`/`BitmapHandle` `SafeHandle` pattern in
`bindings/dotnet/TinkerPdf.cs`, and maps non-`Ok` statuses to `PdfException`
carrying `tpdf_last_error_message`.

**Parity is byte identity.** Two scripts, fixed inputs: *fill-and-save* opens
a form fixture, fills named fields, saves incrementally (7.5.6 — the original
bytes must survive as a prefix, the property signatures depend on per 12.8.1).
`testdata/` holds no form today (simple-text, outline-3level,
permissions-noprint, encrypted-aes256), so milestone 1 commits
`testdata/form-fields.pdf` — the hand-written `/AcroForm` shape
`edit.rs`'s own inline test fixtures already use, with one widget
deliberately lacking `/Rect` so the skipped-widget report leg is exercised,
not just possible. *Build-a-document*
registers a base font and an image, draws two pages, sets info and outline,
finishes.
Both run with pinned `WriteOptions`, and the encrypted variant passes fixed
entropy — the one input that would otherwise vary — so ruling 4's determinism
contract makes byte-identical output the *expected* result, not a lucky one.
The facade-direct hash is committed beside the three synthesised-document
byte-hashes already in `crates/tinker-pdf/tests/determinism.rs` (the
`*_is_the_same_bytes_on_every_target` trio); each smoke test grows a
write leg printing `WROTE sha256=<hex>`, and a `cargo xtask bindings-parity`
leg compares the four lines against the committed hash and exits nonzero on
any mismatch or any missing line. The saved bytes are additionally re-opened
through this engine's strict structural validator, which is what keeps four
byte-identical outputs from being identically wrong — under ruling 13 that
check is first-party, so it cannot be absent and cannot be skipped.

**Packaging.** No new artifacts: the wheel, npm tarball and NuGet package
already staged by gap 26 carry the new symbols. `cargo xtask release` (dry run
default), `nuget-stage` and `release.yml` are untouched except that their
smoke gates now include the write legs, so a package that ships a read-only
`cdylib` fails its own release pipeline.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size (S/M/L/XL) |
| --- | --- | --- | --- |
| 1 | Facade primitives: `EditCheckpoint`/`restore`, `begin_page`/`push_page`; closure APIs become sugar; `testdata/form-fields.pdf` fixture | `cargo test -p tinker-pdf-cos` green with `transaction`'s existing rollback tests unchanged; new tests: restore is idempotent, nested checkpoints restore their own start, `begin_page` snapshot timing equals `add_page`'s, and sugar-vs-primitive byte-hash equality on a built document; the fixture is committed with a `testdata/README.md` line, and filling its `/Rect`-less widget's field returns exactly one `SkippedWidget` | M |
| 2 | C ABI editor surface: `TpdfEditor`, `TpdfCheckpoint`, `TpdfBuffer`, `TpdfFillReport`, page ops, fill, save; `TpdfStatus` appended | `cargo test -p tinker-pdf-ffi` covers: fill-and-save via FFI byte-equal to facade-direct; checkpoint/restore round-trip; spent and null handles refused with statuses, never dereferenced; every new `tpdf_*_free` null-tolerant; incremental save's original-bytes prefix asserted | M |
| 3 | C ABI builder surface: `TpdfBuilder`, `TpdfPageBuilder`, push/finish over `Option<T>` boxing | FFI build-a-document byte-equal to facade-direct in `cargo test -p tinker-pdf-ffi`; double-finish and push-after-finish return `SpentHandle` with a naming error message; the new `#[repr(C)]` types and `extern "C"` signatures added to [bindings.md](../features/bindings.md)'s contract listing and the .NET P/Invoke transcription, reviewed and committed | M |
| 4 | Python and JS wrappers: `PyEditor`/`PyBuilder`, `PdfEditor`/`PdfBuilder`, transaction sugar over the two primitives | `bindings/python/tests` and `bindings/js/tests` run both scripts and print `WROTE sha256=<hex>`; a raising body inside the Python context manager and a throwing JS callback each leave the editor restored, asserted by re-saving and hashing | M |
| 5 | .NET wrapper: five new `SafeHandle`s, `Editor`/`Builder` classes, status-to-`PdfException` mapping | `bindings/dotnet/tests/Smoke` prints `DOTNET-SMOKE: WROTE sha256=<hex>`; disposing the document before the editor still saves correctly; a finalizer-only teardown (no explicit `Dispose`) leaks nothing under the smoke's existing run | M |
| 6 | Parity + validation + CI: committed write hashes in `determinism.rs`, `cargo xtask bindings-parity`, strict-validator leg, release smoke gates extended | `cargo xtask bindings-parity` exits nonzero on any hash mismatch or absent `WROTE` line; every saved artefact passes the strict structural validator; `release.yml`'s smoke jobs run the write legs on every platform they already cover | M |

## Dependencies

- Milestone 1 gates everything (ruling 11: facade first); 2 and 3 need only 1;
  4 needs 1; 5 needs 2 and 3; 6 needs all.
- The gap 26 packaging pipeline (`xtask release`, `nuget-stage`,
  `release.yml`) and the three smoke tests it drives — extended, not rebuilt.
- The determinism suite (`crates/tinker-pdf/tests/determinism.rs`,
  [verification.md](../verification.md)) for the committed write hashes;
  [features/determinism.md](../features/determinism.md) documents the claim.
- The strict structural validator ([verification.md](../verification.md))
  for the validation leg.
- [docs/features/bindings.md](../features/bindings.md) (referenced by
  `tinker-pdf-ffi/src/lib.rs` today) grows alongside; the projections still owed are listed in
  [ROADMAP.md](../ROADMAP.md).

## Risks

| Risk | Mitigation |
| --- | --- |
| Consuming Rust APIs (`finish`, `push_page`) become FFI double-frees | Handles box `Option<T>`; consuming calls `take()`, spent handles refuse with `SpentHandle`, free stays symmetric and null-tolerant — asserted in milestone 2/3 tests |
| Checkpoint pair reintroduces the silent-misuse `transaction()` was designed against | No open state exists by construction: checkpoints are values, restore is idempotent, drop is inert; the three managed wrappers ship the closure sugar so callers rarely touch the pair raw |
| `TpdfStatus` growth breaks the frozen C ABI | Append-only discriminants, existing 0–7 frozen; a `tinker-pdf-ffi` test asserts the numeric values of all pre-existing variants |
| Byte-identical parity is brittle against legitimate writer changes | Hashes live in one place (`determinism.rs`) and the four surfaces are compared to *it*, so a writer change is one recorded update, not four flaky suites; the strict validator is what keeps "identical" from meaning "identically wrong" |
| Encrypted output non-reproducible across surfaces | Entropy is caller-supplied by design; parity passes fixed bytes, and no binding is permitted a randomness default (ruling 11) |
| Skipped widgets flattened into success or failure across FFI | `TpdfFillReport` carries each `SkippedWidget` with its widget `ObjRef` and `WidgetDefect` (ruling 10); smoke tests assert the report crosses non-empty on the `/Rect`-less widget `testdata/form-fields.pdf` carries for exactly this |
| wasm memory growth invalidates borrowed views mid-edit | Write APIs return copies (`TpdfBuffer` on the C ABI, owned `Vec<u8>`/`bytes` in wasm and Python); the only aliasing view remains the read side's `viewUnsafeUntilNextAllocation`, whose detachment `node_smoke.mjs` already demonstrates |
