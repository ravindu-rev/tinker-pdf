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
binding stays a 1:1 projection with no logic of its own. *The read half*,
below, states how the facade's read-only items cross — lists, borrowed data,
values with arms, warnings — and the recipient callback public-key opening
needs, for every step of SU-12 to apply.

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
- No callbacks into host code on the write surface. `FontProvider` stays the
  byte-blob `SimpleFontProvider` projection `tpdf_document_set_fonts` is; no
  host closure runs inside the engine on this surface, so no unwind crosses
  the boundary. A key the engine does not hold crosses as one callback, as
  ruling 20 rules (H-28's answer): signing, [ROADMAP](../ROADMAP.md) row
  SG-09, and unsealing, *The read half*'s recipient callback, on SG-09.4's
  machinery, with script host requests (FJ-17) and warning and progress
  callbacks (SU-21) beside it.
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
6. Parity and validation: the recorded write hashes,
   `cargo xtask bindings-parity`, run locally with `--require-all` once every
   surface is built, the strict-validator leg and the release smoke gates.

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

## The read half

*For [ROADMAP](../ROADMAP.md) row SU-12.* The sections above cross the write
surface; most of what SU-12 owes is the facade's **read-only items** — the
values a document, a page or a verdict hands back — and they cross by a
handful of rules, stated once here so that each step of SU-12 applies them
rather than inventing a shape. `cargo xtask bindings-coverage`
([features/bindings.md](../features/bindings.md), *Coverage*) holds every
surface to every item; these rules are how an item it owes gets projected.
Ruling 11 binds every one of them: a surface carries what the facade hands
back, in the facade's order, with nothing added, cached or defaulted of its
own.

### Scope

- **Lists and trees**: a facade value that is a sequence — a `Vec`, an
  iterator, a tree of nodes.
- **Borrowed data**: bytes and text the facade lends rather than hands over.
- **Values with arms**: enums whose variants carry payloads, verdicts with
  findings and abstentions inside them.
- **Warnings**: the warnings a value carries (ruling 10), with their
  provenance.
- **Read-side options**, and **errors** from a read.
- **Retained objects**: a value the facade keeps for repeated use.
- **The recipient callback** public-key opening needs
  (`Document::authenticate_with_recipient`), beside SG-09's signing,
  timestamper and entropy callbacks.

### Non-goals

- **Laziness across the boundary.** An iterator crosses collected; nothing on
  a surface holds a document's internals open between calls.
- **Counted work** (`Stats`): the refusal table's row stands until a step owes
  it.
- **Warnings and progress during an operation**: SU-21's callbacks (ruling
  20). A value's own warnings cross with it, as below.
- **Typed exceptions per status**: SU-14.1's, over the statuses these rules
  add.

### Lists and trees

- **The C ABI hands back a list handle**, on the `TpdfSignatures` pattern: the
  call fills an opaque handle holding the engine's own copy of the value, so
  the list outlives the document it was read from; `tpdf_<items>_count` gives
  its length and one accessor a field reads entry `i`
  (`tpdf_<item>_<field>(list, i, out)`); `tpdf_<items>_free` releases it and
  accepts null. An index past the end is the family's own status where it has
  one (`NoSuchSignature`) and `BadArgument` otherwise, never a null on `Ok`.
- **An iterator crosses collected**, in the facade's order and under the
  facade's bound where it has one (`MAX_*`): the list is what the facade's
  iterator yields, taken once at the call.
- **A tree crosses as its nodes in pre-order**, each with its parent's index
  (the root's is `UINT32_MAX`) and its depth, so the structure tree, the
  structured text and an outline cross as one list, and a host rebuilds the
  tree in one pass.
- **Python and JavaScript hand back the language's own list** of frozen
  objects, each built at the call with every field the facade's value has:
  PyO3 classes with `get_all`, wasm-bindgen classes with getters.

### Borrowed data

- **On the C ABI a borrow lives as long as its handle**, the
  `tpdf_bitmap_data` rule: a pointer into a list's storage with its length,
  valid until the list is freed, never caller-freed. Text the facade lends is
  handed back as a fresh UTF-8 string freed with `tpdf_string_free`, a null
  on `Ok` meaning the field is absent; bytes, which may hold a zero, always
  cross with their length.
- **Python copies into `bytes`**, or hands a `memoryview` over an object it
  owns; **JavaScript always copies** into a `Uint8Array` — wasm memory growth
  detaches a view across any engine call, the refusal table's wasm
  `view()` row.

### Values with arms: enums, verdicts and findings

- **A C-like enum crosses as its number**, append-only and pinned by a test,
  as `TpdfStatus` is; a caller-supplied one is an `int` checked on entry, as
  every enum a caller hands over is.
- **An enum whose arms carry payloads crosses as its arm and an accessor a
  payload field**: `tpdf_<value>_kind` gives the arm's number, and
  `tpdf_<value>_<arm>_<field>` each of that arm's fields, refusing with
  `BadArgument` — and a message naming the arm the value is — for another
  arm's. This is how SU-12's verdict and finding steps carry the payloads the
  refusal table's "payloads inside a signature enum" row keeps off the C ABI
  today; that row leaves with the steps that apply this rule.
- **A verdict is a value with lists inside it**: a handle with the verdict's
  own fields and, for each list it holds — findings, abstentions, timestamps,
  gaps — a count and the list's per-index accessors, each entry's arms by the
  rule above. A finding keeps its rule, its clause, its kind and the object it
  names.
- **Python and JavaScript** give such a value a `kind` — the facade's variant
  name in snake case — and the payload's fields as attributes, `None` or
  `undefined` where the arm has none; a verdict's lists are lists of those.

### Warnings

- **A value's warnings cross with it** as a warnings list on the
  `TpdfWarnings` pattern: for each, its kind's number, the object it names —
  number and generation, `0 0` for none (ruling 10's provenance) — its message,
  and its payload by the arm rule. A call handing back a value and warnings
  puts the warnings on the value's handle (`tpdf_<value>_warnings`), so the two
  are never read from different calls.
- **Python and JavaScript** give the value a `warnings` attribute, a list of
  warning objects with `kind`, the object and the message.

### Read-side options and errors

- **An options struct crosses as a `#[repr(C)]` struct whose first field is
  its size in bytes**, filled with the facade's defaults by
  `tpdf_<options>_init`; the engine reads the fields the size covers and takes
  the facade's defaults for the rest, so a field the facade appends to its
  `#[non_exhaustive]` options breaks no caller built against the older header.
  Python takes keyword arguments defaulting to the facade's values, and
  JavaScript an options class whose constructor holds them, as
  `PdfWriteOptions` does.
- **An error crosses as a status and `tpdf_last_error_message`**: a family of
  typed errors appends its arms to `TpdfStatus` once, each keeping its number,
  and the error's own sentence is the message; Python raises and JavaScript
  throws with it.

### Retained objects

A value the facade keeps for repeated use — a `DisplayList` — crosses as a
handle of its own, `Send + Sync` as `TpdfDocument` is, usable from any thread
and freed independently of its document, since it holds its own share of it.

### The recipient callback

`Document::authenticate_with_recipient(&dyn Recipient)` opens a public-key
document (7.6.5) by handing the caller each sealed file key with the
identifier of whose it is, for the one private-key operation the engine does
not hold the key for.

- **It crosses on SG-09.4's callback machinery, as ruling 20 has the signing
  callback cross** — H-28's answer — so the read half waits on no decision.
  Ruling 20 gives a key the engine does not hold one callback, so unsealing is
  that callback's second operation rather than a second callback: the key
  vtable SG-09.1 designs and SG-09.4 lands carries an `unseal` entry beside
  `sign`, from its first version, and a host holding a key for one operation
  leaves the other null, which the engine answers with the operation's status.
- **The entry**: `int32_t (*unseal)(void *context, const uint8_t *key, size_t
  key_len, const uint8_t *issuer_and_serial, size_t issuer_and_serial_len,
  const uint8_t *subject_key_identifier, size_t subject_key_identifier_len,
  uint8_t *out, size_t out_capacity, size_t *out_len)`, exactly one of the two
  identifiers non-null, as `Recipient::unseal` has them. It answers 0 with the
  unsealed bytes written into `out` — the engine's buffer, 256 bytes, which
  holds `/Adobe.PubSec`'s 24-byte seed and permissions and any payload a
  transport recipient unseals — and 1 for "not mine". `unseal` has no third
  answer, and ruling 11 keeps the C ABI from inventing one, so any other
  return, or a length past the capacity, is read as "not mine"; a host that
  failed keeps its own error and the engine reports `NoMatchingRecipient` when
  no envelope is answered.
- **The rules every callback of the machinery keeps**: called synchronously,
  on the thread that made the call, never after it returns and never from
  another thread; the context is the host's and borrowed for the call, as the
  facade's `&dyn Recipient` is; no unwind crosses — each binding catches its
  language's exceptions on its side; the callback may not call into the engine
  with the same document, which is borrowed for the call; and the unsealed key
  is a secret operand, copied into the engine and zeroised once the file key
  is derived (ruling 20).
- **The door**: `tpdf_document_authenticate_with_recipient(doc, key, out)`
  writes a `TpdfAuthLevel`, and `PubSecError`'s arms are statuses of their own,
  appended.
- **Python** takes a callable `(key, issuer_and_serial, subject_key_identifier)
  → bytes | None`, called with the GIL held since the call is synchronous; an
  exception it raises is answered as "not mine" and raised again once the
  facade's call returns. **JavaScript** takes a function of the same shape,
  `Uint8Array`s in, a `Uint8Array` or `undefined` out, called synchronously.
  **.NET** marshals a delegate (`Marshal.GetFunctionPointerForDelegate`, kept
  alive for the call), **Go** a `cgo.Handle` in the context behind an exported
  trampoline, **Ruby** a `Fiddle::Closure`, and **Java** an upcall stub in an
  arena closed after the call.

### Which steps follow

Each applies the rules above to its area, and its owed rows leave
`features/bindings.md` when it lands:

| Step | What it projects | The rules it applies |
| --- | --- | --- |
| SU-12.5 | associated files and their readers, page output intents, links | lists; borrowed data |
| SU-12.6 | the reporting finish and the archival builder | lists; values with arms (`ArchivalRefusal`) |
| SU-12.7 | editor pages and annotations, `Page::annotation_list` | lists; values with arms (the annotation payloads); warnings |
| SU-12.9 | `check_save`, `check_decrypt`, the save options | values with arms (`SaveRefusal` with its `Permissions`); options |
| SU-12.12 | stamping, form XObjects, `Page::images` | lists; borrowed data (the samples) |
| SU-12.13 | tagged writing's tables, namespaces and identifiers | options; lists |
| SU-12.14 | a signature's `/Contents`, `/M`, `/ContactInfo`, `/Filter`, warnings and `modifications` | borrowed data; warnings; lists |
| SU-12.15 | timestamps and validation data | lists; values with arms (`TimestampVerdict`); warnings |
| SU-12.16 | public-key opening and sealing | the recipient callback |
| SU-12.18 | the standalone openers | options; errors |
| SU-12.19 | `to_svg`, `display_list`, recoding | retained objects; warnings; options |
| SU-12.20 | PDF/A, PDF/UA and PDF/X verdicts | values with arms; lists |
| SU-12.21 | redaction and search | lists; options |
| SU-12.22 | the structured-text writers, reading order, tables, font listing | trees; lists; options |
| SU-12.23 | `render_form`, `render_annotation`, the layer listing, the subset report | errors; lists |

### Risks

| Risk | Mitigation |
| --- | --- |
| A list handle copies a large value — a page's images with their samples | The copy is the value the facade hands back, so no surface holds more than the facade's caller would; the handle frees it, and Python and JavaScript hold it once |
| A payload accessor asked for another arm reads garbage | It refuses with `BadArgument` naming the arm the value is; the arm numbers are pinned as the statuses are |
| An options struct grows and breaks a caller built against the older header | Its first field is its size, and the engine reads only what the size covers |
| A host's recipient fails and the failure reads as "not mine" | That is the facade's own contract for `unseal`; the host keeps its error, and the engine's `NoMatchingRecipient` says no envelope was answered |
| A host calls back into the engine from inside `unseal`, or from another thread | The machinery's rules forbid both, as the streaming vtable's thread-safety rule is stated; each binding's callback runs on the calling thread and is released when the call returns |
| The unsealed key outlives its use | It is a secret operand: copied in, used to derive the file key, and zeroised (ruling 20) |
