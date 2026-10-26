# Bindings

Seven surfaces over one facade: a C ABI; Python and JavaScript/WebAssembly
directly over the facade; and .NET, Go, Ruby and Java over the C ABI — with
Swift beside them as source that has never been compiled. Ruling 11 ([rulings.md](../rulings.md)) is the whole design — **the
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
`FieldUnreadable`, `SpentHandle`, `EditRefused`, `SourceMiss`,
`ScriptRefused`, `StreamUnreadable`, `FormDataRefused`) and
`tpdf_last_error_message` carries the detail. Those numbers *are* the ABI —
a C caller compares them against literals and the .NET binding against an
`int` — so they count from 0 in the order listed, each keeps its number, and
a new status is appended rather than inserted. Unit tests pin them: one names
every status's number, and one holds the list and its length, so a variant
added without a line is caught by the count rather than by somebody
remembering. Another pins every discriminant of the signature enums and the
write ones, because those are transcribed by hand into
`bindings/dotnet/TinkerPdf.cs`.

**Every enum a caller hands over is an `int`, and is checked.** The enums a
caller supplies — `TpdfPixelFormat`, `TpdfWriteMode`,
`TpdfDestKind`, `TpdfTargetKind`, `TpdfImageKind`, `TpdfInfoKey`,
`TpdfTrapped`, `TpdfLabelStyle`, `TpdfPageBoundary`, `TpdfSanitiseList`,
`TpdfFieldValueKind`, `TpdfBlendMode`, `TpdfSoftMask`, `TpdfMaskKind`,
`TpdfDeviceSpace`, `TpdfTilingType` and `TpdfTagText` — cross as an `int`
parameter or struct field, with the enum still in the header naming the
numbers, and a number the enum does not declare is `BadArgument` with a
message naming the enum and the number, exactly as a null pointer is. The
reason is Rust's, not C's: a `#[repr(C)] enum` holding a number it does not
declare is undefined behaviour the moment it exists, before any `match`
could refuse it, and Ruby and Go pass whatever integer their caller gives
them. Enums only the engine writes — the status, out pointers, fields of a
struct it fills — keep their enum type, since every value written is declared.
Two details follow from "checked" rather than invented: a field the call
does not read is not judged (a blend mode whose `has_blend_mode` is 0, a
mask kind under a soft mask that is not a group), and
`tpdf_sanitise_report_count`, which returns a count rather than a status,
answers 0 for a list number that names no list while the two report
accessors refuse it. `src/raw_enum_tests.rs` pins every enum's range and
every entry point's refusal.

The threading rule differs on the two halves, and it is not a caveat but a
consequence. A `TpdfDocument` may be used from any thread because every read
borrows an immutable, shared `Document`. A `TpdfEditor`, `TpdfBuilder` or
`TpdfPageBuilder` is **mutable state**: the calls take `&mut`, so two threads
in one handle is the same data race it would be in Rust, and no C ABI can
stop it. One handle per thread, or the caller's own lock; freeing stays safe
from any thread.

**The functions** fall into groups: open and render, streaming,
validating, writing, running form scripts (`tpdf_editor_recalculate` and the
calls of its report, `tpdf_editor_formatted_value`, `_keystroke` and
`_validate`), reading signatures, the read surface, document operations, the
forms surface, the builder's graphics resources and tagged writing.
Validating is the strict validator the write surface leans on
(`tpdf_document_validate`, `tpdf_defects_count`, `tpdf_defect_rule`,
`tpdf_defect_message`, `tpdf_defects_free` — an owned `TpdfDefects` on the
`TpdfSignatures` pattern, so it outlives the document). Open and render:
`tpdf_version`,
`tpdf_last_error_message`, `tpdf_document_open` / `_free` / `_page_count` /
`_is_encrypted` / `_authenticate` (returning a `TpdfAuthLevel` of `None`,
`User` or `Owner`) / `_may_print` / `_set_fonts`, `tpdf_page_size` /
`_text` / `_render`, `tpdf_string_free`, and `tpdf_bitmap_width` /
`_height` / `_stride` / `_data` / `_free`.

**The streaming seam** (Annex F): `tpdf_document_open_streaming`,
which takes a `TpdfSourceVtable` of `len` / `read` / `free` plus an opaque
context, and `tpdf_document_is_streamed`. Function pointers rather than a
struct carrying data, because a vtable is what every host language already
knows how to build — `ctypes` in Python,
`Marshal.GetFunctionPointerForDelegate` in C#, three functions in C.

Two things about it are worth stating here rather than only in the header.
**The callbacks must be thread-safe**: the engine calls `read` from whatever
thread is rendering and may call it from several at once, which is the
`Send + Sync` on `ByteSource` being inherited across the boundary, and
nothing on either side can check it. And a `read` that refuses gives
`TpdfStatus::SourceMiss` rather than `NotAPdf`, with the range named in
`tpdf_last_error_message` — the two demand opposite responses, since a host
told "not a PDF" stops and a host told this fetches and calls again.
Collapsing them would make the seam unusable for the thing it exists for.

**Reading signatures** (12.8). Reading only: the signing side is not
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

**The read surface beyond pages** is in `src/read.rs`, each function one
facade call. `tpdf_document_info` takes a
`TpdfInfoKey` for the eight `/Info` text entries, `tpdf_document_trapped`
answers a `TpdfTrapped` whose `Absent` is the key missing and whose `Unknown`
is the document saying `/Unknown` — the facade's `Option<Trapped>` carries
both and a three-arm enum would merge them — and `_pdf_version` and
`_xmp_metadata` (a `TpdfBuffer`) follow. **Page labels are a list handle**,
`tpdf_document_page_labels` → `TpdfPageLabels` with
`tpdf_page_labels_count`, `tpdf_page_label_text` and `tpdf_page_labels_free`,
because the facade answers `page_labels()` — every label at once, the number
tree walked and each range numbered for the whole document — so one walk
builds the handle and every indexed read is a lookup. A document with no
labels is an empty handle, and an index past the end is `BadArgument` like
every other list. Go's `PageLabels()`, Ruby's `page_labels`, Java's
`pageLabels()` and .NET's `ReadPageLabels()` (a `PageLabels` with `Count`
and `Label(index)`) each make the one walk; Python's `page_labels()` and
JavaScript's `pageLabels()` call the facade directly. The outline, a
page's links, the attachments and the warnings cross as owned handles on the
`TpdfSignatures` pattern — `TpdfOutline`, `TpdfLinks`, `TpdfAttachments`,
`TpdfWarnings`, each with `_count`, index accessors and `_free` — so each
outlives the document it came from; `TpdfAttachments` holds its own clone of
the document for that reason, since listing reads no bytes and
`tpdf_attachment_data` reads them only when asked. The outline is
**flattened** with each entry's depth (`OutlineItem::flatten`'s own answer),
because a tree of handles is a tree of frees. A destination reads back as a
`TpdfDestinationRead` whose view is the write side's `TpdfDestination`, NaN
for `null` exactly as there, so a link written through
`tpdf_page_builder_link` and read through `tpdf_link_action` is the same
struct; its three arms (`Explicit`, `Named`, `Uri`, and `Absent` for none)
are never collapsed (ruling 6), and a name or URI crosses as a borrowed
pointer and length, because 12.3.2.3 makes a name a byte string. A link's
action is a `TpdfActionKind` — all six of `Action`'s arms, `/Launch`
reported and never run — with its own bytes beside it. A warning crosses as
its offset, its object, its stable slug and the facade's sentence.

Two answers are kept apart throughout, as they are on the signature surface:
**null on `Ok` is the document not saying**, and an index past the end is
`BadArgument`. The read surface's own status is `StreamUnreadable` (16): an
attachment whose `/EF` stream is named and does not decode is not the same
answer as one that names no stream, and the engine's reason, with the object,
is in `tpdf_last_error_message`. The bytes come from `CosDocument::
stream_decoded` on the stream reference `Attachment` hands back — the route
the facade's own documentation names — so the facade needs no accessor of its
own for it.

**The document operations** are in `src/docops.rs`, each function one
`DocumentEditor` call. `tpdf_editor_set_page_labels` takes an array of
`TpdfPageLabelRange` (a `TpdfLabelStyle` and a nullable prefix);
`tpdf_editor_attach_file` a `TpdfEmbeddedFile` of pointers and a length, its
dates a `TpdfDate` of eight `int32_t`s — every field that wide so a
hand-written binding has no padding to guess at, and a month of 300 refused
as `BadArgument` rather than truncated into another date, while a month of
13, which fits the byte and is no month, is the facade's refusal,
`EditRefused` — and it hands back the new file
specification's reference; `tpdf_editor_set_outline` consumes outline entries
exactly as the builder's does. The typed `/Info` setters are
`tpdf_editor_set_info` over the six text keys of `TpdfInfoKey` and
`tpdf_editor_set_info_date` over the two date keys (the other kind of key is
`BadArgument`), with `tpdf_editor_set_trapped` and
`tpdf_editor_set_xmp_metadata`; each answers the facade's `MetadataSync` as a
`TpdfMetadataSync`, because an `/Info` entry and an XMP packet that disagree
are a document that says two things and the caller is owed the warning.
`tpdf_editor_set_page_boundary` takes a `TpdfPageBoundary` — `set_bleed_box`,
`set_trim_box` and `set_art_box` are that call with the boundary named — and
`tpdf_page_boundary` reads one back through `Page::boundary`.
`tpdf_editor_sanitise` takes a `TpdfSanitise` of four flags and hands back a
`TpdfSanitiseReport` whose two lists, chosen by a `TpdfSanitiseList`, give
each entry's `TpdfRemoval`, the object it was removed from or deleted (or the
trailer), the `/S` of a removed action and, for a removed entry, every step
of its path — a key's bytes or an array position counted in the array as it
was. Where the facade answers `Result`, its refusal's own sentence crosses
with `EditRefused`; where it answers `bool` or `Option`, the call and its
argument are named, as for every other edit. `set_viewer_preferences` is not
here: `ViewerPreferences` is eighteen optional entries, five enums and a list
of page ranges, a sub-surface of its own, and is owed.

Python's `Editor` has the same as `set_page_labels([(first, style, prefix,
start)])`, `attach_file(...)` returning `(number, generation)`,
`set_outline`, `set_title` to `set_producer`, `set_creation_date` and
`set_modification_date` over `(year, month, day, hour, minute, second,
offset)` tuples, `set_trapped`, `set_xmp_metadata` — each returning
`"alone"` or `"other-half-unchanged"` — `set_page_boundary` and its three
siblings, and `sanitise(javascript=, actions=, embedded_files=, metadata=)`
returning a `SanitiseReport` whose `removed` and `deleted` carry every field;
`Document.page_box(index, boundary)` reads one back. JavaScript has them in
camelCase, a `PdfPageLabelRange` class for the ranges, dates as
`[y, m, d, h, mi, s]` arrays with a seventh element for a stated offset, and
the report's lists as plain objects. .NET has them over the C ABI as
`SetPageLabels`, `AttachFile`, `SetOutline`, `SetInfo`, `SetInfoDate`,
`SetTrapped`, `SetXmpMetadata`, `SetPageBoundary` and its siblings,
`Sanitise(SanitiseOptions)` and `Document.PageBox`.

**The forms surface** is in `src/forms.rs`, each function one
facade call. `DocumentEditor::add_field` takes a `NewField` whose kind has
four arms with four payloads, so it crosses as four functions —
`tpdf_editor_add_text_field` (a nullable value and a `/MaxLen` behind a
presence flag), `_add_checkbox`, `_add_radio_group` (an array of
`TpdfRadioButton`, a pointer, a page and four doubles) and
`_add_choice_field` (an array of option strings, combo and editable flags) —
each also taking `NewField`'s `flags` and `font_size`, whose `0` and `0.0`
are `NewField::new`'s own, and handing back the new field's reference. A
union of every arm's fields would be a struct where most fields mean nothing
for any one call, which is the layout a hand-written binding gets wrong.
Its refusal, `AddFieldError`, crosses as `EditRefused` with the facade's own
sentence. Form data is an owned `TpdfFormData` on the `TpdfSignatures`
pattern: `tpdf_document_form_data` (`FormData::from_fields` over the
document's field tree), `tpdf_form_data_read_fdf` and `_read_xfdf`, or
`tpdf_form_data_new` with `_add_field` and `_set_source` to build one;
`_to_fdf` and `_to_xfdf` write it out as a `TpdfBuffer`; `_count`,
`_field_name`, `_field_value_kind` (a `TpdfFieldValueKind`: `None`, `Text`,
`State`, `Many`), `_field_value_count`, `_field_value`, `_source`,
`_warning_count` and `_warning` (a `TpdfFormDataWarningKind` with the key and
the field) read it; `_free` releases it; and `tpdf_editor_apply_form_data`
applies it through `form_data::apply`, with `tpdf_editor_fill_field`'s three
outcomes — a status means nothing was written, `Ok` hands back a
`TpdfFillReport`. Its status is `FormDataRefused` (17), for a file
the reader will not read in the format asked for, or a value XML 1.0 cannot
carry, distinct from `NotAPdf` because an FDF never claimed to be a PDF.
Python has them as `Editor.add_text_field(name, page, rect, value=,
max_len=, flags=, font_size=)` and its three siblings, each returning
`(number, generation)`, `Editor.apply_form_data`, `Document.form_data()` and
a `FormData` class (`read_fdf`, `read_xfdf`, `fields` as `(name, kind,
values)`, `add_field`, `source`, `warnings` as `(kind, what, field)`,
`to_fdf`, `to_xfdf`); JavaScript the same in camelCase, with a
`PdfRadioButton` class and `flags` a `BigInt` as `rotatePage`'s degrees are;
.NET `AddTextField` and its siblings, `ApplyFormData`, `ReadFormData` and a
`FormData` class; Go, Ruby and Java the same over the C ABI.
`SigningTarget::NewVisibleField` with `SignatureAppearance` is not here: it
is a signing target, and signing is a host callback this surface does not
take (below).

**The builder's graphics resources** are in
`src/graphics.rs`, each function one `DocumentBuilder` or `PageBuilder` call.
`tpdf_builder_new_with_version` declares a version (each part a `u32` no
wider than a byte, so a hand-written binding has no narrow integer to
pass), `tpdf_builder_clear_image_resources` stops later pages inheriting the
images registered so far, and `tpdf_builder_add_named_font` takes glyph
names and widths from a first code, which `tpdf_page_builder_encoded_text`
then draws by code with a character and a word spacing.
`tpdf_builder_add_ext_gstate` takes a `TpdfExtGState` — both alphas as
**NaN for absent**, the blend mode behind a presence flag, and the `/SMask`
as a `TpdfSoftMask` whose `Absent` and `None` are different answers (no
entry inherits the mask in force; `/None` turns it off) with the group
mask's form name and backdrop beside it — that `tpdf_ext_gstate_init` fills
as `ExtGState::default()`, because a zeroed struct is two alphas of 0.
`tpdf_builder_add_form` and `tpdf_builder_add_tiling_pattern` take the box,
an optional `/Matrix` as six doubles or null, a nullable
`TpdfTransparencyGroup` and a `TpdfTilingType` respectively, and the
content stream; `tpdf_page_builder_set_ext_gstate`, `_form`,
`_set_fill_pattern` and `_set_stroke_pattern` invoke them, and
`_set_bleed_box` sets the page's box. Where the facade answers `false` —
an alpha out of range, a mask over a form that is not a group, a degenerate
box, a zero step, a name nothing is registered under — the call is
`EditRefused` naming it, and the document is what it would have been
without it. Python has `DocumentBuilder.with_version`, `add_named_font`,
`add_ext_gstate(resource, fill_alpha=, stroke_alpha=, blend_mode=,
soft_mask=, mask_form=, backdrop=)`, `add_form(resource, bbox, content,
matrix=, group=)`, `add_tiling_pattern` and `clear_image_resources`, and
`PageBuilder.set_bleed_box`, `encoded_text`, `set_ext_gstate`, `form`,
`set_fill_pattern` and `set_stroke_pattern`, every enum by its arm's name;
JavaScript the same in camelCase with a `PdfExtGState` built by setters;
.NET, Go, Ruby and Java the same over the C ABI. Shadings and shading
patterns are not here: a `Shading` carries a `Function`, which is recursive
and has a PostScript calculator arm, and is a sub-surface of its own.

**Tagged writing** is in `src/tagging.rs`, `PageBuilder::tagged`
spelled without its closure. A `TpdfTag` is an owned `Tag` built a property
at a time — `tpdf_tag_new` with the structure type, `tpdf_tag_set_text` over
a `TpdfTagText` (`/T`, `/Lang`, `/Alt`, `/ActualText`, `/E`),
`tpdf_tag_set_id`, `tpdf_tag_set_key` (the key and order that make halves
drawn apart one element) and `tpdf_tag_keep_empty` — and
`tpdf_page_builder_open_tag` borrows it, so one tag may open several
elements; `tpdf_page_builder_close_tag` closes the innermost. An element
open when its page is pushed is reopened on the next page begun, as the
facade's own `open_tag` does. `tpdf_builder_set_language` writes the
catalog's `/Lang` and `tpdf_builder_map_role` the `/RoleMap`. An open past
the deepest nesting the reader walks, a close with nothing open, and a role
mapping the facade refuses are `EditRefused`, and write nothing. Python has a
`Tag(kind, title=, lang=, alt=, actual_text=, expansion=, id=, key=,
keep_empty=)` class with `PageBuilder.open_tag` / `close_tag` and
`DocumentBuilder.set_language` / `map_role`; JavaScript a `PdfTag` built by
setters, its key and order `BigInt`s; .NET, Go, Ruby and Java the same over
the C ABI. A tag's table attributes, namespace and associated files, and
`continue_at`, `map_role_in`, `add_namespace` and `duplicate_element_ids`,
are not here: each takes or returns a shape of its own.

**The write surface, and its shape.** What separates it from the read
surface is shape, not capability: `DocumentEditor::transaction(|tx| ..)` and
`DocumentBuilder::add_page(w, h, |page| ..)` take closures, and a closure does
not cross this boundary. Ruling 11 answers that — the facade carries the
closure-free equivalent, and the C ABI is a mechanical wrapping of a Rust API
that already exists: `DocumentEditor::checkpoint` / `restore` and
`DocumentBuilder::begin_page` / `push_page`, with both closure APIs
implemented as callers of them
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
this surface has. `DocumentBuilder::finish(self)` and `push_page` and
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
field for field **but one**, with the booleans and the version pair widened to
integers so a hand-written P/Invoke has no packing to guess at. The one is
`deduplicate_streams`: the struct does not carry it, because a field added
there is a layout change the header and every binding pin, so a C caller's save
crosses with it at its default, off — owed to the [roadmap](../ROADMAP.md)'s
bindings row. And
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
the GIL, so a thread pool over pages is actually parallel. The read surface
is `metadata` (a `Metadata` whose absent entries are `None`), `pdf_version`,
`page_labels()`, `outline()` (nested `OutlineItem`s, each with a
`Destination` whose `kind` is "explicit", "named" or "uri"), `links(page)`,
`attachments()` (each with `data()`), `xmp_metadata()` and `warnings()`.
Signatures are `signatures()` and `verify_signatures(TrustAnchors, at=None)`:
every enum the facade answers with is its arm's name, and every arm's payload
— the revision a coverage ends at, the defect that makes it suspicious, why a
check was not made, the subject a chain names, a short key's bits — a sibling
attribute, which the C ABI cannot carry (a C enum has no payload) and a Python
object can. A `Signature` also carries `/Contents`, `/M`, `/ContactInfo`,
`/Filter`, `/FieldMDP` and its lenient-read warnings, which the C ABI does
not. One wheel per
platform, not per interpreter: `abi3-py39`, and the release workflow
asserts the `abi3` tag is in the filename.

**JavaScript / wasm** (`bindings/js`, wasm-bindgen directly over the
facade). `PdfDocument`, `pageCount`, `isEncrypted`, `authenticate`,
`mayPrint`, `pageWidth` / `pageHeight`, `pageText(i)`, `setFonts`,
`renderPage(i, scale)`; the read surface as `metadata`, `pdfVersion`,
`pageLabels()`, `outline()`, `links(page)`, `attachments()`,
`xmpMetadata()` and `warnings()`, with `PdfView` the one class both
directions share (`linkToPageView`, `setPageTargetView`); and signatures as
`signatures()` and `verifySignatures(PdfTrustAnchors, at)`, payloads and all,
as in Python — `at` a number of seconds, truncated as `Math.trunc` does, and
NaN, an infinity or a number past 2^63 thrown back rather than judged as the
epoch or the end of time; an attachment's declared `size` is a `BigInt`, the
facade's `i64` exactly, since the document writes that number and nothing
bounds it at 2^53;
`bitmap.data()` copies, `bitmap.viewUnsafeUntilNextAllocation()` aliases
wasm linear memory and silently becomes zero-length when a later allocation
grows the memory — `node_smoke.mjs` renders,
takes a view, renders larger and asserts the view's length is 0. The
dangerous call has the warning in its name. **ESM only, `--target web`**:
the `nodejs` target's CommonJS loads the `.wasm` with a synchronous read a
browser cannot do, so a dual package would be two builds of the engine that
can diverge, and ruling 11's point is that a binding has nothing to diverge
*with*. Node ≥ 18 runs the same file by handing `init` the bytes. The
package is `tinker-pdf-js`, name and version derived from `Cargo.toml` so
`cargo xtask versions` has no extra manifest to police. `release.yml` gates
the gzipped `.wasm` at 2.5 MiB, with every predefined CMap in; `cmap-predefined`
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

**Go, Ruby and Java over the C ABI** (`bindings/go`, `bindings/ruby`,
`bindings/java`). Three thin wrappers, each nothing but the header in its
language's spelling: one method per C call, or a loop of calls over a list
the engine hands back, every enum carrying the C numbers, every
handle closed by its owner's `Close`/`close` (safe twice), and every string
and byte array handed back a copy that outlives its handle. None decides a
default: a save takes the options `tpdf_write_options_init` filled in, a
view the engine's own `tpdf_destination_init_fit`, and an encrypted save the
caller's 48 bytes of entropy. Go and Java call every function the header
declares; Ruby every one but `tpdf_document_open_streaming` (below). Each has
the parity program (below) and a smoke program that
renders blank-then-inked and then calls, once each, every declaration the
parity program does not reach — authentication against the two encrypted
fixtures, the editor's page operations, fields, checkpoint and restore, the
form-script calls, an encrypted save reopened with its password, an embedded
and subset face, the page builder's colours, crop box and raw operators —
so a declaration transcribed wrongly fails there rather than in a caller.

**Every arm of every enum is held to the header, not only the arms a script
uses.** The parity scripts send most enums across by one arm — the graphics
script one blend mode, one mask kind, one device space, one tiling type —
so a wrong number for any other arm would pass `bindings-parity` and every
smoke. Go and Swift name the header's own constants (`SoftMaskAbsent SoftMask =
C.TPDF_SOFT_MASK_ABSENT`), so the compiler checks each number; Ruby and
.NET write the numbers out and Java passes `ordinal()`, so its declaration
order is the number. `crates/tinker-pdf-ffi/tests/binding_enums.rs` reads
the committed header and the four bindings' sources as text and holds every
enum each carries to the header's: the same arms, none missing and none
extra, each with the header's number — Java's by position, Go's by the
constant it names, which must be its own arm and never a literal — and
Swift's statuses each to its own constant. The two enums no binding
keeps as a table, `TpdfTargetKind` and `TpdfSanitiseList`, are a literal 0
or 1 at the one place each crosses, and both arms of each cross in a parity
script. Python and JavaScript name arms by string over the facade, so no
number crosses there; the same file holds each of their string-to-arm
lines, both directions, to the rule that the string is the arm's own name
(the page boundaries' `"trim"` for `TrimBox` the one stated exception).

- **Go** is cgo, and the only one of the three that compiles against the
  header rather than transcribing it, so cgo lays out every struct and
  every enum constant is the header's own.
  `bindings/go/tinkerpdf.go` links `target/release` with that directory as
  its run path; `go run ./cmd/smoke testdata/simple-text.pdf FACE.ttf` and
  `go run ./cmd/parity testdata/form-fields.pdf` from `bindings/go`. Every
  C call runs with its goroutine's OS thread locked until the error message
  has been read, because `tpdf_last_error_message` is per thread and a
  goroutine may otherwise move between the call and the read. Streaming is
  `OpenStreaming(Source)`: the vtable is built in C (it crosses by value and
  its members are C function pointers), its three callbacks are exported Go
  functions, and the context is a `cgo.Handle`, deleted by the `free`
  callback.
- **Ruby** is Fiddle, from the standard library, so there is no gem and no
  compiler: `ruby -Ilib test/smoke.rb …` and `ruby -Ilib test/write_parity.rb
  …` from `bindings/ruby`, loading `TINKER_PDF_LIB` or the release library in
  this checkout. Structs cross as packed strings; their layouts are the C
  crate's, pinned by `crates/tinker-pdf-ffi/tests/layout.rs` with
  `offset_of!`, so a field moved there fails there. The one function it does
  not call is `tpdf_document_open_streaming` (below).
- **Java** is the Foreign Function and Memory API (`java.lang.foreign`),
  written to the part of it that is the same in JDK 21 — where it is a
  preview API, compiled with `--enable-preview --release 21` and run with
  `--enable-preview` — and JDK 22, where it is final and **Java 22 and later
  drop the flag**. CI runs JDK 21, so the 22 claim rests on the API's
  documented equivalence, not on a run; `cargo xtask
  bindings-parity` reads `javac -version` and picks the flags. Every downcall
  in `Native.java` is generated from the header, one line each; structs are
  written field by field at the offsets `layout.rs` pins; streaming is three
  upcall stubs in an arena the document owns, released after it.
  `--enable-native-access=ALL-UNNAMED` silences the restricted-method
  warning, and the library is `-Dtinkerpdf.library=` or `TINKER_PDF_LIB`.

**Swift** (`bindings/swift`) is **unverified source**: it has never been
compiled. It is a SwiftPM package whose
`CTinkerPdf` module imports the committed header through a shim (not a
copy, which would be a second transcription to drift), a `TinkerPdf` target
covering the core — open, text, render, validate, authenticate, the form
fill and save, the builder — and a `Smoke`
executable written to the same blank-then-inked pattern. It has no parity
program, is not in `bindings-parity` and has no CI job; the read surface,
document operations, signatures and streaming are owed, and so is a first
build.

**`set_fonts` everywhere.** The engine bundles no faces and reads no font
directories ([fonts](fonts.md)), so a document that embeds none extracts its
text perfectly and draws none of it. The `FontProvider` seam is projected
across every surface, and every smoke test renders *twice* — blank
without a face, inked with one — because "a bitmap of the right size came
back" passes on a build whose renderer does nothing at all.

**Write parity is byte identity.** Two scripts with every
input pinned — *fill-and-save* opens `testdata/form-fields.pdf`, fills its
damaged field, its undamaged control field, a checkbox and a radio group and
saves incrementally; *build-a-document* registers a base font and an image,
draws two pages through `begin_page`/`push_page`, sets `/Info` and an outline
and finishes — run on every surface, and every surface must print the same
hash. That is the evidence for ruling 11: surfaces disagreeing would mean one
of them added something. The image both scripts draw is computed from a formula rather
than read from a file, so every language produces the same 64 bytes with no
fixture between them — a parity suite whose surfaces read the same *file*
proves only that they can read a file.

**The document operations are byte identity too.** *document-ops* opens
`testdata/outline-3level.pdf` and, through one editor, sets page labels,
attaches a file with a description, a MIME type and a creation date, sets
`/Title`, `/Author`, `/CreationDate` and `/Trapped`, writes an XMP packet,
sets a trim box and a bleed box and replaces the outline, then saves;
*sanitise* takes everything `Sanitise::ALL` names out of that artefact and
saves again, and writes its report down as *sanitise-report* — removed
entries with their holders and paths, deleted objects — so what went is
compared as well as what is left.

**And read parity is text identity.** A further script, *read-surface*, opens
three documents — `testdata/outline-3level.pdf` with five bytes in front of
its header, which the reader tolerates and reports, a two-page document each
surface builds itself with two links, a nested outline and an `/Info` title
outside ASCII beside an author that is empty rather than absent, and the
document-ops artefact — and writes down everything the read surface says
about each: version, page count, the eight `/Info` entries, `/Trapped`, page
labels, every page's five boundaries, the outline with its destinations,
every link with its action, every attachment with a hash of its bytes, the
XMP packet's hash and, last, every warning. The text is specified
byte for byte in `crates/tinker-pdf/examples/write_parity.rs`: strings and
byte strings as hex, numbers as the sixteen hex digits of their IEEE 754 bits,
because every language formats `1.5` differently and a parity check that
tolerated "close" in a coordinate would be measuring the formatters. Each
surface prints `READ sha256=` of it. *signatures* opens three documents the
signature tests commit — an ECDSA P-256
signature, an `adbe.pkcs7.sha1` one and an RFC 3161 document timestamp —
and a fourth made from the first, and writes down every signature as read
and every verdict twice, anchored to the document's own root (or to nothing,
for the timestamp, which is what reaches `no-anchors`), judged at no instant
and at the epoch. The fourth, `ecdsa-p256-altered`, is `ecdsa-p256.pdf` with
the first `verdict path` in its bytes changed to `verdict PATH` — inside the
signed range and the same length — so it is the one verdict whose document
digest (`differs`) and signature check (`verified`) disagree, and a surface
that read those two answers from each other's accessor disagrees with the
rest. Only what the C ABI carries goes into the text, so every
surface can produce it; the payloads Python and JavaScript carry besides are
asserted in their own scripts against what the fixtures are known to be.

**And the options a save takes.** The scripts above save with the defaults
but the mode, so a surface that dropped linearization, the version, object
streams, compression or garbage collection on the way to the engine would
agree with every surface that did not. Two more scripts cover them:
*save-options* opens the document-ops artefact, deletes page 1 and
appends `0 0 m 100 100 l S` to page 0 (something for garbage collection to
drop and an unencoded stream for compression to compress), then saves a
rewrite declaring PDF 2.0 with object streams, compression and garbage
collection; *save-linearized* is the same save linearized. It is a second
script rather than one save with everything on because the linearizer sets
object streams and compression aside — a linearized save with either turned
off is the same bytes — so a single save would not see two of the options.
Encryption is the one left to the smoke programs, which reopen an encrypted
save with its password, because the artefact would need the password to be
validated here.

**And the forms surface, both halves.** *forms* opens the form fixture and,
through one editor, creates a field of every kind `add_field` makes — a text
field with a value and a `/MaxLen`, a required check box that starts ticked,
a radio group of two buttons with the second selected, a combo box with a
font size and a list box — then reads `form-fields.xfdf` (one of the
hand-written fixtures under `crates/tinker-pdf/tests/form_data/`), applies
it, and saves. *form-data* writes down each created field's reference, the
widget the apply wrote a value for and could not draw (the fixture's
`/Rect`-less one, `7.0`), and then, for eight sets of form data — the forms
artefact's own fields, the four fixtures, `hierarchy.fdf` altered three ways
so the reader leaves the `value-unreadable`, `unnamed` and `tree-cut`
warnings the fixtures never reach, a set built a field at a time and a set
holding a character XML 1.0 cannot carry — its source, every field with its
value's shape and strings, every warning, and the hashes of the data written
back as FDF and as XFDF (`refused` for the last); and last, that bytes which
are neither format are refused by each reader. The altered FDF is there for
the reason the altered signed document is: without it, a surface that
spelled one of those three warnings as another would agree with every surface.

*graphics* builds a two-page document declaring PDF 2.0 with every one of
the builder's graphics resources used: a standard font under a named
encoding drawn by code with both spacings, an isolated grey transparency
group with a matrix and a plain form, a graphics state with both alphas, a
blend mode and a luminosity soft mask over the group with a backdrop and a
second that only turns the mask off, a tiling pattern with a matrix filling
and stroking, a bleed box, and an image the second page does not name, the
list being cleared before it.

*tagged* builds a two-page tagged document through `open_tag` and
`close_tag`: the catalog's `/Lang`, a custom type mapped to `H1` with a
title, a paragraph with a language and an actual text opened on page one and
closed on page two, a figure with alternate text, a span with an expansion
and an identifier, an element kept empty and two keyed halves of one
element drawn out of their reading order (a key that lost its order would
read them the other way round); a close with nothing open is refused on
every surface and changes
nothing.

`cargo xtask bindings-parity` is the gate, and it is built around two different
failures. A **mismatch** is the one everybody thinks of. An **absent line** —
a surface that ran, exited zero and printed no `WROTE sha256=` at all — is the
one that gets shipped, because a script that silently does nothing looks
exactly like a passing one; both exit non-zero. A surface whose artefact is not
installed on the machine is **SKIPPED**, by name and with the reason, and the
run says how many ran and how many did not (ruling 13), so a job that quietly
found no interpreter cannot pass. `--require-all` turns every skip into a
failure, and that is what `ci.yml`'s `bindings-parity` job passes after
building and installing every artefact and setting up Go, Ruby and JDK 21. The
three bindings over the C ABI are skipped, by name, when the release library is
not built or their toolchain is not on `PATH`; Java's compile is a step that
must succeed first, so a binding that does not compile fails rather than skips.
The Go surface runs with the library search path set to `target/release`
alone, because under `cargo run` it otherwise holds `target/debug/deps`, where
a test build can leave an older `libtinker_pdf_ffi`, and the loader prefers the
search path to the program's run path.

**Agreement is not enough, and this is what makes it enough.** Seven
byte-identical outputs tell you nothing if all seven are wrong. So every surface
re-opens its own artefact through this engine's strict structural validator and
refuses to print a hash for a file that is not clean — which is why `validate()`
is projected on every surface (`tpdf_document_validate` and the `TpdfDefects`
handle on the C ABI, `Document.validate()` in Python and JavaScript,
`Document.Validate()` in .NET, and the same call in Go, Ruby and Java). Under ruling 13 that validator is
first-party, and that is precisely why it can be a gate here rather than an
external step somebody might not have installed.

The recorded hashes live in `xtask/src/parity.rs` rather than beside the
synthesised-document hashes in `crates/tinker-pdf/tests/determinism.rs`. Those
are a *rendering* claim's fingerprints, moved only by the renderer or the
synthesiser, and interleaving a bindings hash among them would make a writer
change read as a determinism regression to whoever opened that file next. One
place, and every surface compared to *it* rather than to each other, so a
legitimate writer change is one recorded update instead of seven flaky suites.

**Packaging, built and dry-run, nothing published.** `cargo run -p xtask --
release` walks wheel, npm package, NuGet package and crates in an order
computed from the manifests. The dry run is the default and `--execute` is
the flag that publishes, because a half-published release cannot be
retracted from crates.io. **Nothing is published to any registry**: the
facade does not freeze before 0.1.0, and a published package invites
dependence on an API that is explicitly unstable. In `release.yml` a tag
builds every artefact and publishes none — publishing needs a
`workflow_dispatch` carrying `publish: true` — and a final job counts the
three abi3 wheels, the npm package's `.wasm` and the `.nupkg` and fails unless
all are there.

`cargo publish --dry-run` resolves dependencies against the live index, and
nothing named `tinker-pdf-*` is published there, so every crate above the
leaves fails a plain dry run with `no matching package named
tinker-pdf-crypto` — which reads exactly like a broken manifest and is not
one — and the plain dry run reports those steps as unprovable.

`cargo run -p xtask -- release --local-registry` proves them. Each crate's
registry dependencies are patched at `target/package/<name>-<version>/` — the
unpacked archive the previous step left behind — so every crate is *verified
against the same bytes its dependents would download*, which is nearer to a
real publish than building against this checkout would be. The patch set is
everything published before that crate: one level deep is not enough, because a
packaged dependency has registry dependencies of its own, and the whole
workspace is too much, because nothing is packaged when the first crate runs;
`each_crate_is_patched_against_the_ones_before_it` in `xtask/src/release.rs`
pins the width. Under it nothing is unprovable, and the one step skipped is
`dotnet nuget push`, which has no harmless form.

## API

```python
import tinker_pdf
doc = tinker_pdf.Document(open("file.pdf", "rb").read())
doc.set_fonts(open("DejaVuSans.ttf", "rb").read())
bitmap = doc.render(0, dpi=150.0)
memoryview(bitmap.data)

editor = doc.editor()
# Three outcomes, not two: this raises when nothing was written, and returns
# a list — empty or not — when the value was written.
for widget in editor.fill_field("name", "Ada Lovelace"):
    print(f"not drawn: {widget}")          # 7 0 R: no usable /Rect (12.5.2)

with editor.transaction():                 # checkpoint, `with`, restore
    editor.set_checkbox("agree", True)
    editor.select_radio("colour", "red")

data = editor.save(mode="incremental")     # bytes; the original is a prefix
assert tinker_pdf.Document(data).validate() == []

builder = tinker_pdf.DocumentBuilder()
builder.add_base_font(b"F1", b"Helvetica")
page = builder.begin_page(200.0, 200.0)    # the resource snapshot is *here*
page.text(b"F1", 14.0, 20.0, 170.0, "Page one")
builder.push_page(page)                    # consumes the page
builder.set_outline([tinker_pdf.OutlineEntry("Page one", page=0)])
pdf = builder.finish()                     # consumes the builder
```

```js
import init, { PdfDocument, PdfWriteOptions } from 'tinker-pdf-js';
await init();
const doc = new PdfDocument(bytes);
const bitmap = doc.renderPage(0, 1.0);
const pixels = bitmap.data();          // a copy; safe to keep

// There is no editor.transaction(callback): an exported method borrows its
// `this` for the whole call, so JavaScript running inside one that touched the
// same editor would hit wasm-bindgen's "recursive use of an object detected".
// Checkpoint, host control flow, restore — three lines, in the host.
const editor = doc.editor();
const mark = editor.checkpoint();
try {
  editor.fillField('name', 'Ada Lovelace');
} catch (e) {
  editor.restore(mark);
  throw e;
} finally {
  mark.free();
}

const options = new PdfWriteOptions();   // the engine's defaults, not zeros
options.setMode('incremental');
const saved = editor.save(options);      // a copy, never a view into wasm
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

using var editor = document.CreateEditor();   // outlives `document`
foreach (var widget in editor.FillField("name", "Ada Lovelace"))
{
    Console.WriteLine($"not drawn: {widget}");  // 7 0 R: no usable /Rect (12.5.2)
}

editor.Transaction(() =>                      // checkpoint, try, restore
{
    editor.SetCheckbox("agree", true);
    editor.SelectRadio("colour", "red");
});

var saved = editor.Save(new WriteOptions { Mode = WriteMode.Incremental });

using var builder = new DocumentBuilder();
builder.AddBaseFont("F1"u8.ToArray(), "Helvetica"u8.ToArray());
using (var page = builder.BeginPage(200.0, 200.0))   // snapshot is *here*
{
    page.Text("F1"u8.ToArray(), 14.0, 20.0, 170.0, "Page one");
    builder.PushPage(page);                          // consumes the drawing
}
var pdf = builder.Finish();  // a second Finish is Status.SpentHandle
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

**The header is `crates/tinker-pdf-ffi/include/tinker_pdf.h`**, generated by
cbindgen 0.29 from `crates/tinker-pdf-ffi/cbindgen.toml` and committed,
because a C, Go or Swift caller needs it in the tree and `cargo build` runs
nothing but rustc. The `extern "C"` items under `crates/tinker-pdf-ffi/src/`
remain the contract and the header is their spelling. Two things keep a
committed generated file from drifting: `tests/header.rs` reads both as text
and fails when an export is missing from the header or a declaration names
nothing — it spawns nothing (ruling 13) — and CI's `bindings` job regenerates
the header and fails on any difference, which is what catches a signature that
moved under an unchanged name. The .NET binding's P/Invoke declarations and
the Ruby binding's `extern` lines are worked transcriptions of it, the Java
binding's downcalls are generated from it, Go compiles against it and Swift
imports it as a module. What a transcription cannot see from the header is a
struct's padding, so `tests/layout.rs` pins the size and every field offset of
every struct a binding packs by hand.

Each of the first three bindings' READMEs ([js](../../bindings/js/README.md),
[python](../../bindings/python/README.md),
[dotnet](../../bindings/dotnet/README.md)) carries its build, smoke-test and
packaging commands; the Go, Ruby, Java and Swift bindings carry theirs in
their sources' opening comments and in the section above. None of the four is
packaged.

## Refused by name

| What | How it shows | Why | See |
| --- | --- | --- | --- |
| `ImageData::Compressed` | `TpdfImageKind` has `Jpeg`, `Rgb8` and `Gray8` and no fourth arm | it carries a `CompressedImage` whose colour space holds a palette slice and whose filter holds its own parameters, so projecting it is a sub-surface rather than a struct. It exists for the CBZ synthesiser, which must not decode 200 pages at open — an engine-internal path with no host at the other end. A host holding already-compressed bytes has `Jpeg`, which is the same idea for the one codec hosts actually hold bytes in | [creation](creation.md) |
| A tag's `table`, `namespace` and `associated_file`; `PageBuilder::continue_at`; `DocumentBuilder`'s `map_role_in`, `add_namespace` and `duplicate_element_ids` | not projected (`PageBuilder::tagged` **is**, as the `open_tag`/`close_tag` pair, with a tag's text properties, identifier, key and keep-empty flag, `set_language` and `map_role`) | owed rather than refused: each takes or returns a shape of its own — `TableAttributes` with its header lists and spans, a `NamespaceId` handle a builder issues, a `NewAssociatedFile`, a list of identifiers | [creation](creation.md) |
| The rest of `PageBuilder` — `glyphs`, `text_pieces`, `shading` — and `DocumentBuilder`'s `add_cid_font`, `glyph_run`, `add_shading`, `add_shading_pattern` | not projected (the graphics states, forms, tiling patterns, named fonts, encoded text, bleed box, version and `clear_image_resources` **are**) | owed rather than refused: `glyphs` and `glyph_run` draw `Glyph`s through a composite font (and `text_pieces` takes them beside a simple font's codes, in `TextPiece`s), which `add_cid_font` registers from a font program, and `add_shading` takes a `Shading` built on a `Function` — recursive, with a PostScript calculator arm — a sub-surface of its own. `tpdf_page_builder_raw` is the escape hatch that keeps them reachable in the meantime | [ROADMAP.md](../ROADMAP.md) |
| `DocumentEditor`'s `import_page`, `keep_pages`, `flatten_annotations`, `add_annotation`, `reset_form`, `set_field_values`, `set_calculated_values` | not projected (`recalculate` **is** projected, as `tpdf_editor_recalculate`) | the same: owed, each with a shape of its own — a second document, a slice of indices, a `Dict`, a `Recalculation` | [ROADMAP.md](../ROADMAP.md) |
| The graphics-writing surface: `Target::Named` and both `add_named_destination`s; `DocumentBuilder::add_layer`, `PageBuilder::optional` and `DocumentEditor::set_layer_visible`; `add_separation_color_space`, `add_device_n_color_space`, `set_fill_tint` / `set_stroke_tint` and `ImageColorSpace::Tint`; `DocumentEditor`'s `stamp`, `add_resource`, `add_form` and `import_page_as_form`; `Page::images` | not projected | owed rather than refused: each takes or returns a shape of its own — a `LayerId` handle, a `Function::Calculator` program, `DeviceNAttributes`, a `StampPlacement` and a form reference, a `PageImage` with its samples, masks and placements — and `optional` takes a closure, so it needs a closure-free pair on the page handle as `tagged` does. Ruling 11 makes each a debt | [ROADMAP.md](../ROADMAP.md) |
| The device-independent colour writers: `add_icc_color_space` with `set_fill_icc` / `set_stroke_icc` and `ImageColorSpace::Icc`, and the CIE writer — `CieSpace`, `DocumentBuilder::add_cie_color_space`, `PageBuilder::set_fill_cie` / `set_stroke_cie`, `ImageColorSpace::Cie` and `ImageColorSpace::Lab` | not projected | owed rather than refused: `CieSpace` is an enum whose three arms each carry their own parameters — a white and a black point, one gamma or three, a column-by-column matrix, an `a*`/`b*` range — which a C enum has no room for, so it needs a struct of its own; the setters take a component slice whose length the registered space decides, as `set_fill_tint` does; and the image arms are colour spaces of `ImageData::Compressed`, as `ImageColorSpace::Tint` is. Ruling 11 makes each a debt | [ROADMAP.md](../ROADMAP.md) |
| Signing: `save_signed`, `Signer` | no `tpdf_*` entry point takes a callback | a signer is a host callback; in-engine keys, deferred two-phase signing and a signing callback are a roadmap row | [ROADMAP](../ROADMAP.md) SG-09 (design/bindings-write.md) |
| The payloads inside a signature enum — which revision, which defect, whose certificate, how many bits | the enum arm crosses, the payload does not | a C enum has no payload, and a struct invented here to carry one would be this crate spelling something the facade already spells (ruling 11) | [signatures](../design/signatures.md) |
| A signature's `/Contents` blob, `/M`, `/ContactInfo`, `/Filter` and its lenient-read warnings on the C ABI (Python and JavaScript carry them), and `Signature::modifications` everywhere | not projected | owed rather than refused: each is a shape of its own — raw bytes, a date, a list of changed objects — rather than another string or enum | [signatures](../design/signatures.md) |
| The signature and public-key surface: `DocumentEditor::save_timestamped` with `Timestamper` and `TimestampRequest`; `add_validation_data` with `ValidationData`; `Document::security_store` with `SecurityStore` and `SecurityStoreWarning`; `PublicKeyEncryption::seal` and `DocumentEditor::save_sealed`; `Verdict::timestamps` with `TimestampVerdict`; `Signature::validation_key` | not projected | owed rather than refused. `Timestamper` is a host callback, which the write design keeps off the C ABI as it keeps `Signer`, and `seal` takes an `EntropySource`, which is another; the rest take or return shapes of their own — lists of DER blobs, a store of object references, a per-token verdict with its own enums. Ruling 11 makes each a debt | [ROADMAP.md](../ROADMAP.md) |
| Counted work: `RenderOptions::stats`, `Bitmap::stats` and `Stats` | not projected; `tpdf_page_render` and every binding's render count nothing and hand back no stats | owed rather than refused: the C ABI's render takes a scale and a format rather than the options, so asking needs an entry point of its own, and `Stats` is `#[non_exhaustive]` and gains a counter with each row that counts one, so a struct carrying it across needs a layout that can grow. Ruling 11 makes it a debt | [performance](../design/performance.md) |
| `DocumentEditor::check_save` and `check_decrypt` with `SaveRefusal` | not projected; a save through the C ABI or a binding writes what it is asked to, so a rewrite of an encrypted input asking for no encryption writes it decrypted, as the facade's own `save` does | owed rather than refused: each is a question asked before a write, answered by an enum one arm of which carries a `Permissions`. Whether the save doors themselves refuse is an open question on the ROADMAP; if they do, every surface that saves through them refuses with them | [encryption](encryption.md) |
| CommonJS build | none; ESM only | two builds of the engine can diverge; a CommonJS entry that re-exports the one ESM build cannot | [ROADMAP](../ROADMAP.md) SU-14 |
| Holding a wasm `view()` across an engine call | the view becomes zero-length | wasm memory growth detaches the buffer; use `data()` | — |
| A security handler the engine lacks | `TpdfStatus::UnsupportedHandler` | public-key encryption is absent | [encryption](encryption.md) |
| Streaming in Ruby | `TinkerPdf::Document` has no streaming open; every other function is there | `tpdf_document_open_streaming` takes its vtable **by value**, and Fiddle passes no struct by value; and the engine calls `read` from whatever thread is working, where a Ruby block would run without the GVL. A by-pointer variant on the C ABI would answer the first and not the second | [opening](opening.md) |
| Swift beyond its core | the core only, no parity program, no CI | never compiled; widening unverified source would only widen what nobody has run | [ROADMAP.md](../ROADMAP.md) |
| Published packages | `pip install` / `npm install` / `dotnet add package` do not work, and Go, Ruby, Java and Swift have no package at all | the facade is unstable before 0.1.0 | [ROADMAP.md](../ROADMAP.md) |

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
  every entry point, with every `tpdf_*_free` accepting null.
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
  and fuzz crates are outside the workspace, so CI checks them explicitly).
- Smoke tests run against an *installed* artifact, never the source tree:
  `bindings/python/tests/wheel_smoke.py` against a `pip install`ed wheel,
  `bindings/js/tests/node_smoke.mjs` against an `npm install`ed tarball,
  `bindings/dotnet/tests/Smoke` against a local folder feed with the source
  list cleared so a missing package fails rather than resolving from
  nuget.org. Each asserts the blank-then-inked render.
- The **write** legs run beside them, from the same installed artefacts:
  `write_parity.py`, `write_parity.mjs` and the Smoke program's third
  argument, each printing `WROTE sha256=<hex>` per script, and
  `crates/tinker-pdf/examples/write_parity.rs` doing the same against the
  facade. `release.yml` greps for those lines on every platform its smoke jobs
  cover, so a package that shipped a read-only `cdylib` fails its own
  release pipeline. Each also asserts the leg its language can express and the
  others cannot: the Python context manager and the .NET
  callback-`Transaction` restore on an exception, and the JavaScript three
  liner restores on a throw — every one of them checked by saving before and
  after and comparing hashes, which is the only form of that assertion that
  cannot be faked, and every one of them requiring the exception to *still
  escape*, because a rollback that also hid the reason would be the worst of
  both.
- **The read surface is pinned by the same equality with the facade**
  (`src/read/tests.rs`): a document built in the test with every shape the
  surface reads — an empty `/Author` beside an absent `/Subject`, a nested
  outline with explicit, URI and named targets and a heading with none, links
  of two kinds, page labels, an attachment and an XMP packet — and the shifted
  outline fixture are read through the C ABI and through `Document`, and every
  `/Info` entry, `/Trapped`, version, label, outline entry and destination
  (every view number, NaN against `None`), link rectangle, reference, action
  and its bytes, attachment field and decoded byte, the XMP packet and every
  warning's offset, object, slug and sentence must agree. Around them: an
  index past each list's end is `BadArgument`, a page past the end is
  `NoSuchPage`, the attachments handle reads bytes after its document is
  freed, `the_page_label_handle_holds_its_own_copy_of_every_label` frees the
  document before reading the first of 300 labels and holds every read to the
  facade's, a null handle is refused on every entry point and every
  free accepts null, and the enums' numbers are pinned.
- **The document operations are pinned by byte equality with the facade**
  (`src/docops/tests.rs`): every operation made through the C ABI and the
  same operations against `DocumentEditor` save the same bytes, which the
  facade then reads back as the labels, attachment, title, `/Trapped`,
  packet, trim box and outline asked for; a sanitise through the C ABI of a
  document with a JavaScript open action, an attachment, `/Info` and XMP
  saves the facade's bytes and reports the facade's entries — every removal,
  holder, path step and action — and its object deletions. Around them: a
  boundary read back equal to `Page::boundary` for all five; each refusal
  (no range at page 0, a name already attached, a month out of range, a date
  key to the text setter and the reverse, `Absent` to `set_trapped`, a
  rectangle with no area, a page past the end) writing nothing and naming
  why; a spent outline entry refused on a second `set_outline`; null on every
  entry point; the enums' numbers and `TpdfDate`'s 32 bytes pinned.
- **The forms surface is pinned by byte equality with the facade**
  (`src/forms/tests.rs`): a field of every kind created through the C ABI
  and through `DocumentEditor::add_field` saves the same bytes; a document's
  form data, data built a field at a time and data read from FDF and XFDF
  carry the facade's fields, values and source, and write the facade's FDF
  and XFDF bytes; applying it saves what `form_data::apply` saves, and a
  refused apply writes nothing; every warning crosses as its own kind with
  its strings, from an XFDF and from an FDF that reaches all four arms.
  Around them: each refusal (a taken name, a rectangle with no area, a check
  box on a page past the end with `Off` as its export value, a radio group
  with no buttons) writing nothing and naming why; bytes that are neither
  format, and a value XML
  cannot carry, refused as `FormDataRefused` with the reader's sentence;
  null and out-of-range on every entry point; the enums' numbers pinned; and
  `TpdfRadioButton`'s 48 bytes in `tests/layout.rs`.
- **The builder's graphics resources are pinned by byte equality with the
  facade** (`src/graphics/tests.rs`): a document using every one of the
  calls builds the same bytes through the C ABI and through
  `DocumentBuilder`, declares 2.0 and passes the strict validator; an
  initialised `TpdfExtGState` registers exactly `ExtGState::default()`; each
  refusal (a version part past a byte, names and widths that disagree, a
  first code past 255, an alpha past one, a mask over a form with no group, a
  degenerate box, a zero step, four page calls naming nothing) writes
  nothing, so the finished document equals the facade's built from the
  calls that were taken; null on every entry point and a spent builder are
  refused; the enums' numbers and both structs' layouts are pinned.
- **Tagged writing is pinned by byte equality with the facade**
  (`src/tagging/tests.rs`): the *tagged* script's document built through the
  C ABI equals the facade's, passes the strict validator and reads back
  through `Document::structure` with no warning; one tag handle opens both
  keyed halves; a refused role mapping, a close with nothing open and the
  opens past the depth the reader walks write nothing, so the document
  equals the facade's built from the calls that were taken; null on every
  entry point is refused; `TpdfTagText`'s numbers are pinned.
- **Signatures in Python and JavaScript** are held by the *signatures*
  script's hash, equal to the facade's, and by an assertion leg in each
  script for what only those two carry: an anchor that is not a certificate
  refused and not kept, the anchor's and recognised subfilter's names, DER in
  `/Contents`, one `SignerInfo`, the anchor subject the chain names, an
  `outside-validity` weakness naming a subject when judged at the epoch, and
  `no-anchors` with no subject and no trust when nothing is trusted.
- `cargo xtask bindings-parity` compares every surface against the recorded
  answer in `xtask/src/parity.rs`.
- **Go, Ruby and Java** are held by the parity programs — every recorded hash,
  equal to the facade's — and by smoke programs that render
  blank-then-inked and then call every declaration the parity programs do
  not. `crates/tinker-pdf-ffi/tests/layout.rs` pins the
  hand-packed structs. The Swift package
  has never been compiled.
- `bindings/js/demo/verify.mjs` drives the browser demo in headless
  Chromium and checks the ink's bounding box is the shape of a line of
  text rather than a smear or a stray pixel.
- The release workflow asserts the `abi3` wheel tag and greps the `.nupkg`
  for all three RIDs ([verification](../verification.md)).
