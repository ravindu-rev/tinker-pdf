# Capability tier — editing, document operations and creation

[The roadmap](../ROADMAP.md) says how a step runs, what comes next, and which dependencies cross tiers. Each row below keeps its text as it was planned — Item, Today, Exit criterion, Doc, Depends on — and is cut into steps, each one workflow run.

Parity here means content edited at the level of page objects and text,
every page operation an office user reaches for, colour and image
conversion, revisions and undo, redaction with no residue, and creation by
API and by layout, each held to the strict validator (K-03).

## ED-01 — A page-object editing model

**A page-object editing model**: enumerate a page's objects (text runs, paths, images, shadings, forms, inline images) with their state; change a matrix, colour, blend, dash, text or font; remove, insert, reorder; regenerate the content stream, untouched operators byte-stable where possible

**Today.** `append_content`, and the redaction rewrite path (`cut_stream` in `crates/tinker-pdf/src/redact.rs`, ruling 7), only; content editing "beyond `append_content`" is out of scope (`design/bindings-write.md:47-50`)

**Exit criterion.** an edited page renders as the same page built directly with the builder (fingerprints); a no-op edit leaves the stream byte-identical; every output validates strictly

**Doc:** `design/page-objects.md` (owed) · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-01.1 | design | Write `docs/design/page-objects.md` (scope, non-goals, design, milestones, risks): operator spans reported by the interpreter through the `Device` seam, so ruling 7 holds; objects grouped from them with the state the renderer applies; a token writer whose output re-reads as its input; regeneration as replacements of spans, untouched operators copied byte for byte; and a closure-free facade API over `DocumentEditor` (ruling 11) that reads, edits, inserts, reorders and removes. *Note:* Non-goals point at their rows: reflow and subset extension (ED-02, whose `text-editing.md` builds on this doc), placeholders (ED-03), cleaning and the public filter pipeline (ED-04), objects grafted from another document (ED-30), the bindings (`bindings-write.md`). Risks: the interpreter's leniencies the model inherits (a 64-deep `q` cap, a non-finite `cm` ignored), and text matrices restored exactly only while every number round-trips. | `docs/design/page-objects.md` (new) | The doc exists with its five sections in order and is reviewed; it settles each kind's grouping (a path with its clip and paint, a text run inside its `BT`, `Do`, `sh`, `BI`...`EI`, a form's children), the state reported, each edit's operators and refusals, how a split `/Contents` array is rewritten, what 'where possible' still rewrites, how reads avoid `DocumentEditor::view`'s copy of the file per page, and the object and replacement caps; its milestones name ED-01.2 to ED-01.19's exit tests. | — |
| ED-01.2 | build | A token writer in `tinker-pdf-content` whose output re-reads as the token it was given: names and strings through the escaping the COS writer already applies (`write_name`'s `#xx` of 7.3.5, `write_string`'s escapes of 7.3.4.2, carriage return included), made public in `tinker-pdf-cos` for it; numbers in the shortest plain decimal that parses back to the same `f64`, never in exponent form (7.3.3); arrays, inline dictionaries, and one operation as operands, operator and separator. *Note:* Reuses `crates/tinker-pdf-cos/src/write.rs:170-214` rather than a third copy beside it and `form_data.rs:880`; only the number rule is new, because `write_real`'s six decimals would not restore a text matrix exactly. Untouched operators never pass through it: they are copied as bytes (ED-01.6). The tokenizer's `parse_number` accepts an exponent the writer must never produce. | `crates/tinker-pdf-content/src/emit.rs` (new), `crates/tinker-pdf-content/src/lib.rs`, `crates/tinker-pdf-cos/src/write.rs`, `docs/features/content-and-text.md` | `emit.rs` (new) unit tests `every_token_rereads_as_itself` (names over every byte value, strings over every byte with CR, CRLF and lone parentheses, numbers at the extremes of `f64` and of Annex C) and `an_operation_rereads_as_its_tokens` pass under `cargo test -p tinker-pdf-content`: `Tokenizer` reads each written form back as the same `Token`. Both fail first, and counted injections dropping the `#xx` escape and the `\r` escape each fail them. | ED-01.1 |
| ED-01.3 | fix | Redaction's rewrite writes every operand it re-emits, and every surviving run of a cut `TJ`, through ED-01.2's writer, in place of `write_token` and `escape_into`, which write a decoded name's bytes bare and a carriage-return byte raw inside parentheses, where the tokenizer reads it back as a line feed; both functions are deleted. *Note:* Every operator of a redacted page is re-emitted (`redact.rs:4730-4745`) and the result is written back whatever was cut (`:1214`), so the defect reaches every page `apply` runs on. Counted injections: restore each old function in turn. | `crates/tinker-pdf/src/redact.rs`, `docs/features/editing.md` | `redact.rs` unit tests (new) `an_escaped_name_survives_the_rewrite` (a `Tf` naming a font whose resource name holds a space or a parenthesis, beside a cut run, still resolves after `apply`) and `a_carriage_return_code_survives_the_rewrite` (a `<0D>` run outside the rectangle shows the same codes after `apply`) fail against today's writer and pass after; every existing redaction test passes. | ED-01.2 |
| ED-01.4 | build | The interpreter reports each operator it runs through a new `Device` method with an empty default: its byte range in the stream it was written in (ranges that partition the stream, leading whitespace and comments included, an inline image from `BI` through `EI` as one), its operator and operands, that stream's identity (0 for the page, a form's packed reference, as `Form::stream`) and the graphics state and text matrices in force before it. *Note:* Ruling 7 holds because the spans come from the interpreter's own walk, not a second one. The method is generic and empty, so every existing device compiles it away and renders, extracts and writes SVG exactly as before. The 512-operand cap and the inline-image skip are unchanged. Counted injection: start a range after its leading trivia. | `crates/tinker-pdf-content/src/interpret.rs`, `crates/tinker-pdf-content/src/device.rs`, `crates/tinker-pdf-content/src/lib.rs`, `docs/features/content-and-text.md` | `interpret.rs` unit test (new) `operator_spans_partition_the_stream` (the reported ranges, joined, are the input byte for byte, each operator once, an inline image whole, a form's operators under its identity) fails first, then passes; the new `a_device_without_the_method_sees_what_it_saw` (a `RecordingDevice` transcript unchanged), every render fingerprint in `determinism.rs` and `a_display_list_replays_every_fingerprinted_page_byte_for_byte` hold. | ED-01.1 |
| ED-01.5 | build | Page objects in `tinker-pdf-content`: a device over the operator spans and the drawing calls that groups a stream into objects (a path with its construction, any clip and its paint; a text run, one showing operator inside its `BT`; an image or form `Do`; an `sh`; an inline image), each with its spans and state: CTM, colours resolved and as written, alpha, blend mode, line width, cap, join, miter limit, dash, font, size, text matrix, render mode, layer and marked-content scopes. *Note:* A form's own objects are kept beneath it, in its frame, for ED-01.15. The operators that last set each state parameter are kept by span under the `q` stack, so an edit can restore them as written (ED-01.12). The object cap joins `bounds_ledger.rs` with the test that crosses it (ruling 1); `architecture.md`'s row for the crate names the new role. | `crates/tinker-pdf-content/src/objects.rs` (new), `crates/tinker-pdf-content/src/lib.rs`, `crates/tinker-pdf/tests/bounds_ledger.rs`, `docs/features/content-and-text.md`, `docs/architecture.md` | `objects.rs` (new) unit tests `every_kind_is_enumerated_with_its_spans` (a stream drawing one object of each kind gives six objects in drawing order whose spans hold their operators and no others) and `an_objects_state_is_the_state_it_was_drawn_with` (CTM, colour, alpha, blend, dash and font equal the state a `RecordingDevice` records at its drawing call, and a run's text matrix puts its first glyph where the recorded glyph is) fail first, then pass under `cargo test -p tinker-pdf-content`; a stream past the object cap is counted, not kept. | ED-01.4 |
| ED-01.6 | build | Regeneration in `tinker-pdf-content`: a stream and a list of replacements keyed to objects' spans give the new stream, every byte outside a replaced span copied as it was and every replacement written through ED-01.2's writer; an empty list, or a replacement equal to the bytes it replaces, gives the input back flagged unchanged; overlapping or unordered replacements are refused. *Note:* The unchanged flag is what lets the facade write nothing for a no-op (ED-01.9). Counted injections: re-emit an untouched span through the writer, drop a span's leading trivia. | `crates/tinker-pdf-content/src/splice.rs` (new), `crates/tinker-pdf-content/src/lib.rs`, `docs/features/content-and-text.md` | `splice.rs` (new) unit tests `no_replacement_returns_the_stream_unchanged` (every seed of `fuzz/corpus/content_tokenizer/`, flagged unchanged) and `a_replacement_changes_only_its_span` (the output is the input with that span's bytes swapped and nothing else) fail first, then pass; `overlapping_replacements_are_refused` passes. | ED-01.2, ED-01.5 |
| ED-01.7 | test | A fuzz target over the content layer, `content_objects`: arbitrary bytes are interpreted with the span hook into objects, and it asserts that the spans partition the input, that enumeration stays inside its caps, that a splice with no replacement is the input, and that each operation written back by the token writer re-tokenizes as the tokens it came from. *Note:* It enters `UNSIGNED` beside `content_tokenizer`: a content stream is unframed operator text. Like every target it checks self-consistency, not correctness; its header names `objects.rs` and `splice.rs`'s tests as where correctness lives, and `verification.md`'s counts of what the targets assert move. | `fuzz/fuzz_targets/content_objects.rs` (new), `fuzz/Cargo.toml`, `fuzz/corpus/content_objects` (new), `fuzz/README.md`, `xtask/src/fuzzaudit.rs`, `docs/verification.md` | `cargo xtask fuzz` is green with the new target, its header's `# What this target checks`, its seed corpus (the `content_tokenizer` seeds' shapes plus a form and marked content) and its row in `docs/verification.md`'s roster, regenerated by `cargo run -p xtask -- fuzz --table`; its seeds replay without a crash under `cargo fuzz run` for twenty seconds, as `ci.yml`'s `fuzz-seeds` job runs them. | ED-01.6 |
| ED-01.8 | build | The facade's read, `tinker_pdf::page_objects`: a page of a `DocumentEditor`, its `/Contents` parts decoded and joined with their boundaries kept and interpreted over `PageResources` so every name resolves as the renderer resolves it, is read into objects carrying ED-01.5's state and each run's font and decoded text, from one editor view however many pages a call reads. *Note:* Its image objects are held to `Page::images` (`crates/tinker-pdf/src/images.rs:328`): each entry of a `PageImage`'s `placements` is an object here with the same transform. Corrects `docs/features/editing.md:98`, which says `append_content` appends to the content array; it replaces `/Contents` with one new stream (`crates/tinker-pdf-cos/src/edit/page_ops.rs:346-373`). | `crates/tinker-pdf/src/page_objects/mod.rs` (new), `crates/tinker-pdf/src/page_objects/read.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/page_objects.rs` (new), `docs/features/editing.md` | `page_objects.rs` (new) `a_built_page_reads_as_what_the_builder_drew`: a page drawn with `fill_rect`, a dashed stroked path, text in an embedded face, `image`, `shading`, `form` and an inline image through `raw` reads as seven objects in drawing order with the colour, matrix, dash, font, size and text the builder wrote, and as the same seven with its content split over a `/Contents` array between operands and operator; `is_dirty()` stays false; both builds. | ED-01.5 |
| ED-01.9 | build | Write-back: `page_objects` applies a list of edits to a page through ED-01.6's splice and puts only the `/Contents` parts whose bytes changed, each in place, both parts where an operator spans two; an unchanged result writes nothing. Lands the harness every later step holds its streams to: 8.2's graphics-object states and operator categories, transcribed, checked over each regenerated stream. *Note:* A part is overwritten in place, as redaction overwrites its stream, so an incremental save carries the changed parts and nothing else. Every document a test in `page_objects.rs` saves goes through K-03's helper. `page_objects.rs` and `mod.rs` are ED-01.8's files. | `crates/tinker-pdf/src/page_objects/write.rs` (new), `crates/tinker-pdf/src/page_objects/mod.rs` (new), `crates/tinker-pdf/tests/page_objects.rs` (new), `crates/tinker-pdf/tests/content_grammar_support/mod.rs` (new), `docs/features/editing.md` | `page_objects.rs` `a_no_op_edit_writes_nothing`: every page of every PDF in `testdata/` and `fuzz/corpus/render_page/` that opens without a password, and builder pages with comments, inline images and a split `/Contents` array, given an empty edit list, leaves `is_dirty()` false; `write.rs`'s unit test `a_replacement_rewrites_only_its_own_part` keeps every other part's object; `the_grammar_harness_rejects_what_8_2_forbids` (`q` inside `BT`, a nested `BT`, an unmatched `Q`) passes; both builds. | ED-01.6, ED-01.8, K-03 |
| ED-01.10 | build | Remove: a path's construction and paint operators dropped, or its paint made `n` where it also clips; an image, form, shading or inline image's operator dropped; a text run replaced by a displacement of its advance so later runs of its text object keep their places; marked-content brackets kept, so the structure tree's sequences still exist. *Note:* The run's displacement is the redaction cut's form (`emit_array`), written through ED-01.2's writer. Counted injections: drop the clip, drop the displacement, drop a bracket. | `crates/tinker-pdf/src/page_objects/remove.rs` (new), `crates/tinker-pdf/src/page_objects/mod.rs` (new), `crates/tinker-pdf/tests/page_objects.rs` (new), `docs/features/editing.md` | `page_objects.rs` `removing_an_object_renders_as_the_page_built_without_it`, per kind: a builder page drawing A, B and C with B removed renders byte-equal (`same_picture`) to one drawing A and C, passes the grammar harness and K-03's helper, keeps every other operator's bytes, and no longer extracts B's text while A's and C's extract where they were; both builds. | ED-01.9 |
| ED-01.11 | build | Matrix, colour, blend and dash for a path, image, form, shading or inline image: its operators wrapped in `q` ... `Q` behind the operators that set the new state (a `cm` taking the CTM in force to the one asked for, a colour space and colour, a `gs` naming an `/ExtGState` with `/BM` registered through `DocumentEditor::add_resource`, a `d`), so nothing drawn after it sees the change. *Note:* A blend edit on a page whose resources are inherited or shared copies them first (`add_resource`'s copy on write, `crates/tinker-pdf-cos/src/edit/stamp.rs:136`). Ordered after ED-01.10 because both plan replacements in `write.rs`. Counted injections: omit the `q`, compose the `cm` the wrong way round, register the `/ExtGState` without `/BM`. | `crates/tinker-pdf/src/page_objects/state.rs` (new), `crates/tinker-pdf/src/page_objects/write.rs` (new), `crates/tinker-pdf/src/page_objects/mod.rs` (new), `crates/tinker-pdf/tests/page_objects.rs` (new), `docs/features/editing.md` | `page_objects.rs` `a_state_edit_renders_as_the_page_built_with_that_state`: per edit and kind, an edited builder page renders byte-equal to the builder page drawn with the new matrix, colour, blend mode or dash, passes the grammar harness and K-03's helper, and keeps every other operator's bytes; an edit restating the state in force leaves `is_dirty()` false; a singular CTM is refused by name; both builds. | ED-01.10 |
| ED-01.12 | build | Matrix, colour, blend and dash for a text run, inside a `BT` where 8.2 admits no `q`: the new state set before the run and the state in force restored after it from the bytes of the operators that set it; a matrix edit ends the text object before the run and opens one after it with the text and line matrices restored exactly, so the runs after it draw where they did. *Note:* Restoring from the setting operators' own bytes, rather than from the resolved colour, keeps a Separation or ICC colour exact. Counted injections: skip the restore, restore the line matrix as the text matrix, open a `q` inside the `BT`. | `crates/tinker-pdf/src/page_objects/text.rs` (new), `crates/tinker-pdf/src/page_objects/state.rs` (new), `crates/tinker-pdf/src/page_objects/mod.rs` (new), `crates/tinker-pdf/tests/page_objects.rs` (new), `docs/features/editing.md` | `page_objects.rs` `a_text_run_state_edit_renders_as_built`: the middle of three runs in one text object, recoloured, given a blend mode, dashed under a stroking render mode and moved, renders byte-equal to the builder page drawing it so, extracts as before, passes the grammar harness (no `q` inside `BT`) and K-03's helper, and the other runs' operators keep their bytes; an edit restating the state in force leaves `is_dirty()` false; both builds. | ED-01.11 |
| ED-01.13 | build | Text and font for a run: the new text encoded in the run's font, or in another font of the page's resources with `Tf` set before the run and restored after it, each character's code read backwards from the font's own mapping and checked forwards (`Font::code_for_char`, `code_for_cid`) with its glyph in the program; a displacement takes up the change of advance so later runs keep their places; a character with no code or no glyph is refused by name. *Note:* Extending a subset or falling back to another face is ED-02's: the refusal names the character and the font, and its row in `editing.md`'s refusal table points at ED-02. Counted injections: skip the forward check, skip the glyph check, drop the displacement. | `crates/tinker-pdf/src/page_objects/text.rs` (new), `crates/tinker-pdf/tests/page_objects.rs` (new), `docs/features/editing.md` | `page_objects.rs` `a_run_given_new_text_renders_as_built` and `a_run_given_another_font_renders_as_built`, over a simple and a composite embedded face: byte-equal renders to the builder pages, `Page::text` reads the new text, and the grammar harness and K-03's helper pass; `a_character_the_font_cannot_draw_is_refused_by_name`, like a run given its own text, leaves `is_dirty()` false; both builds. | ED-01.12 |
| ED-01.14 | build | Insert and reorder: an object read from any page of the same document, or operators the caller supplies that read as whole objects over the page's resources, emitted at a position as one self-contained unit (`q`, the operators that rebuild its state from the state in force there, its own operators, `Q`; a run in a text object of its own) with the resources it names registered on the page; a reorder is that insertion at the new place and ED-01.10's removal at the old. *Note:* Another document's objects need its resources grafted, which is ED-30's graft map, so this reads the same document only. A name the page already uses for something else gets a fresh one through `add_resource`, and the emitted operand follows it. Counted injections: drop the state prologue, keep the source's resource name. | `crates/tinker-pdf/src/page_objects/insert.rs` (new), `crates/tinker-pdf/src/page_objects/mod.rs` (new), `crates/tinker-pdf/tests/page_objects.rs` (new), `docs/features/editing.md` | `page_objects.rs` `inserting_renders_as_the_page_built_with_it` (A and C plus B read from another page render byte-equal to a builder page drawing A, B and C) and `reordering_renders_as_the_page_built_in_that_order` (each kind moved to first and to last) pass the grammar harness and K-03's helper and keep untouched operators' bytes; a move to its own place leaves `is_dirty()` false; each destination the design refuses is refused by name; both builds. | ED-01.13 |
| ED-01.15 | build | Objects inside a form XObject: read as the form object's children, in the form's frame and under the scope the interpreter gives it (8.10.1), and open to every edit above; the form's stream is rewritten where only this placement draws it, and otherwise copied, the copy registered on the page under a new name and this placement's `Do` renamed to it, so no other placement or page changes. *Note:* Whether anything else draws the form is what redaction's `drawn_elsewhere` answers (`redact.rs:2834`), read once per call and never per edit; it moves where both modules reach it rather than a second scan being written, and placements on this page come from the page's own read. Counted injections: rewrite a shared form in place, rename the wrong `Do`. | `crates/tinker-pdf/src/page_objects/forms.rs` (new), `crates/tinker-pdf/src/page_objects/read.rs` (new), `crates/tinker-pdf/src/page_objects/mod.rs` (new), `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/tests/page_objects.rs` (new), `docs/features/editing.md` | `page_objects.rs` `an_edit_inside_a_form_renders_as_the_form_built_so` (a recoloured path and a removed run inside a form drawn once render byte-equal to the builder's form drawn that way) and `a_form_drawn_twice_is_copied_for_the_placement_edited` (the other placement, and another page drawing the form, render as before) pass the grammar harness and K-03's helper; both builds. | ED-01.14, ED-01.3 |
| ED-01.16 | build | Counted work for the model: `page_objects` threads MS-01's work tally through its read, every edit and the save after them, and surfaces it as the opt-in statistics MS-01 gives the facade's other operations, so a guard holds editing every page of a document to work linear in its pages. *Note:* MS-01's A2 shape and ratio, applied to an operation A2 does not list. `DocumentEditor::view` saves and reopens the whole file on every call once the editor is dirty (`crates/tinker-pdf-cos/src/edit/view.rs:48-59`), which is the quadratic this guards against. | `crates/tinker-pdf/src/page_objects/mod.rs` (new), `crates/tinker-pdf/src/page_objects/read.rs` (new), `crates/tinker-pdf/src/page_objects/write.rs` (new), `crates/tinker-pdf/tests/page_objects.rs` (new), `docs/features/editing.md` | `page_objects.rs` `editing_every_page_is_linear` (new): the tally for reading every page of a generated document, removing one object from each page and recolouring another, and saving once is at most 4.4 × at 4 000 pages what it is at 1 000, in `cargo test` on linux, windows and macos and in MS-01's `wasm32-wasip1` guard leg; a counted injection taking a fresh `DocumentEditor::view` for each page fails it. | ED-01.15, MS-01 |
| ED-01.17 | test | A fuzz target over the facade's model, `page_objects`: the input is a page's content over fixed resources (a font, an image, a form, a graphics state, a shading), built, opened and read; a no-op is applied, then removals, recolourings, moves and text changes the input chooses; the result is saved, reopened and read again. It asserts no panic, a no-op that leaves `is_dirty()` false, and a saved file that reopens. *Note:* Ruling 1 for the facade half, as ED-01.7 is for the content half. It enters `UNSIGNED` (its body is operator text, framed by the target), and its header names `page_objects.rs` as where correctness is tested. | `fuzz/fuzz_targets/page_objects.rs` (new), `fuzz/Cargo.toml`, `fuzz/corpus/page_objects` (new), `fuzz/README.md`, `xtask/src/fuzzaudit.rs`, `docs/verification.md` | `cargo xtask fuzz` is green with the new target, its header's `# What this target checks`, its seed corpus and its row in `docs/verification.md`'s roster, regenerated by `cargo run -p xtask -- fuzz --table`; its seeds replay without a crash under `cargo fuzz run` for twenty seconds, as `ci.yml`'s `fuzz-seeds` job runs them. | ED-01.16 |
| ED-01.18 | test | The editing subsystem's determinism fingerprint (K-07): `determinism.rs` builds a page, applies one edit of each kind (remove, matrix, colour, blend, dash, text, font, insert, reorder, an edit inside a form), saves by rewrite and pins the SHA-256 of the bytes in a new test the `ci.yml` determinism legs, native and `wasm-determinism`, grep by name; the edited page joins `GOLDEN`, so its render is reproduced too. *Note:* K-07's rule: the subsystem's fingerprint lands with it, and its entry in K-07's roster with it. The numbers an edit computes (a composed `cm`, a restored text matrix) are what the byte hash pins. The fixture embeds its face (`curvy_font`), so both builds give one hash. A moved hash is a finding, never an edit. | `crates/tinker-pdf/tests/determinism.rs`, `.github/workflows/ci.yml`, `docs/features/determinism.md` | `determinism.rs` `an_edited_document_is_the_same_bytes_on_every_target` (new) and `rendering_is_stable_across_targets`, with the edited page among its fixtures, hold their committed hashes on linux, windows, macos and `wasm32-wasip1`, read to the end in `ci.yml`'s jobs, whose greps name the new test; the test first asserts the document it hashes (each edit's object read back), so a hash over the wrong document fails. | ED-01.15, MS-11, K-07 |
| ED-01.19 | measure | The editing speed row in MS-04's write group: a generated 100-page document read, one object recoloured and one removed on every page, and saved incrementally, sized into MS-04's 20-300 ms window, with its baseline entry; then ED-01 leaves the roadmap and `page-objects.md` gains its as-built section. *Note:* MS-04 splits `benches/` by group; the operation goes in the write group's file with its figure recorded as MS-04 records a new operation's. No target is invented here; one is the owner's (H-77). | `crates/tinker-pdf/benches/engine.rs`, `crates/tinker-pdf/benches/baseline.json`, `.github/workflows/bench.yml`, `docs/ROADMAP.md`, `docs/design/page-objects.md` | `xtask/src/bench.rs`'s `the_committed_baseline_agrees_with_the_tree` holds the bench file, the workflow's guard count and `baseline.json` to one list with the new operation in it, and `cargo bench -p tinker-pdf` reports its time; `docs/ROADMAP.md` no longer holds ED-01. | ED-01.17, ED-01.18, MS-04 |

**Closes the row:** ED-01.8 to ED-01.15 build the model on the facade over the content layer of ED-01.2 and ED-01.4 to ED-01.6, and every edit step holds an edited builder page byte-equal in render to the page the builder draws directly (the fingerprint the exit asks for), every saved output to K-03's strict helper and every regenerated stream to ED-01.9's 8.2 harness, while ED-01.9 and ED-01.11 to ED-01.14 show every no-op leaves the editor clean and the stream byte-identical. ED-01.16 guards the work as linear, ED-01.18 pins the subsystem's fingerprint on four targets and ED-01.19 adds its speed row and takes the row out of the roadmap; ED-01.3 repairs, with the new writer, the one rewrite path the tree already has.

**The tree on 23 October 2026, against this row's text:**

- Ruling 7 (`docs/rulings.md:197-202`) excepts 'the `ContentFilter` rewrite path built for redaction ... which is itself part of the interpreter', but no `ContentFilter` exists (`rg ContentFilter crates` finds nothing): redaction tokenizes on its own in the facade (`crates/tinker-pdf/src/redact.rs:560`, `:4494`), as do `crates/tinker-pdf/src/subset.rs:765` and `crates/tinker-pdf/src/pdfa/content.rs:222`. ED-01 reads operators through the interpreter instead (ED-01.4), so it needs no further exception.
- The redaction rewrite path the Today cell names is lossy for operators it does not cut: every operand is re-emitted through `write_token` (`crates/tinker-pdf/src/redact.rs:4730-4745`, `:5085`), which writes a name's decoded bytes with no `#xx` escape, and `escape_into` (`:5076`) leaves a 0x0D byte raw inside parentheses, which the tokenizer reads back as 0x0A (`crates/tinker-pdf-content/src/tokenizer.rs:242-248`); `apply` writes the result back whatever was cut (`redact.rs:1214`). ED-01.3 fixes it.
- `docs/features/editing.md:98` says `append_content` appends operators 'to a page's content array'; `crates/tinker-pdf-cos/src/edit/page_ops.rs:346-373` instead decodes and joins the existing parts and writes one new stream (`q`, the old content, `Q q`, the operators, `Q`) that replaces `/Contents`, so the page's original streams are left unreferenced rather than byte-stable.
- The tree already enumerates one object kind read-only: `Page::images` (`crates/tinker-pdf/src/images.rs:328`) lists a page's image XObjects and inline images with a transform per drawing (`PageImage::placements`) through the interpreter; the Today cell names only the two writing paths. ED-01.8 holds the model's image objects to it.

## ED-02 — In-place text editing with reflow

**In-place text editing with reflow**: edit a line or paragraph of existing text, reshape with the subset font where its glyphs exist, extend the subset or fall back otherwise, reflow inside the paragraph box, keep tags and `/ActualText`

**Today.** absent — the owner's parity decision of 9 October 2026; content is changed by `append_content` and redaction's rewrite only (ED-01); `rg -i 'edit_text|replace_text' crates tools` finds nothing

**Exit criterion.** an edited paragraph extracts as the new text, draws within its box, keeps its structure element, and its font subset gains exactly the new glyphs; fixtures per script direction

**Doc:** `design/text-editing.md` (owed) · **Depends on:** ED-01, FT-11, FT-30

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-03 — Template text replacement

**Template text replacement**: placeholders in existing content replaced with font fallback and alignment

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'replace_text|template_text' crates tools` finds nothing

**Exit criterion.** fixtures with placeholders replaced and extracted back; alignment computed in the test

**Doc:** — · **Depends on:** ED-01

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-04 — Content-stream cleaning

**Content-stream cleaning**: normalise and pretty-print operators, balance `q`/`Q`, remove top-level state leaks and redundant colour operators, keep marked content; the content-filter pipeline public on the facade

**Today.** the redaction rewrite path (`redact.rs`'s `cut_stream`, ruling 7) is internal

**Exit criterion.** a cleaned page renders byte-identically to the original on every fingerprint fixture and the corpus subset re-run; `q`/`Q` balanced by count

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-05 — Text to outlines

**Text to outlines**: convert all text to vector paths, Type 3 glyphs flattened

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'text_to_paths|text_to_outlines|outline_text' crates tools` finds nothing

**Exit criterion.** the converted page renders identically and extracts no text

**Doc:** — · **Depends on:** ED-04

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-06a — Recolour a document to gray, RGB or CMYK

**Recolour a document to gray, RGB or CMYK** by device-space arithmetic, shadings resampled, output intents optionally removed

**Today.** absent — the owner's parity decision of 9 October 2026; colour is converted for rendering only (`rg -i 'convert_colou?rs|to_grayscale' crates tools` finds nothing)

**Exit criterion.** every colour operator, image and shading converted; the render equals the original render converted in the test (8.6.4's device-space formulas); `tpdf recolour` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-04

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-06b — ICC-managed colour conversion of documents

**ICC-managed colour conversion of documents**: object classes to gray, sRGB, CMYK or the output intent with rendering intents, black-point compensation, device links, pure-black text kept, spots to process (with aliases), rich black, total-area-coverage reduction

**Today.** absent — the owner's parity decision of 9 October 2026; `tinker-pdf-color` converts for rendering only (`rg -i 'convert_document|output_intent_convert' crates tools` finds nothing)

**Exit criterion.** per conversion, the converted values equal the transform recomputed in the test; TAC under the cap on every pixel of the converted render; `tpdf recolour --profile` runs it on `testdata/` fixtures

**Doc:** `design/colour-conversion.md` (owed) · **Depends on:** RC-01, RC-02, RC-20

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-07 — Clip, trim and auto-crop

**Clip, trim and auto-crop**: remove content outside a rectangle; keep or exclude relative to a box with margins; crop to content with a tolerance

**Today.** crop and other boundaries settable (`features/editing.md:89-106`); content is never removed

**Exit criterion.** removed objects absent from the stream; the visible render unchanged; auto-crop box equals the bounding box of marks (RC-17); `tpdf trim` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-04

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-08 — Imposition

**Imposition**: N-up with order and borders, saddle-stitch booklet, grid, cut a page into tiles, poster with overlap and marks

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'imposition|booklet|\bn_up\b' crates tools` finds nothing

**Exit criterion.** each layout's placement matrices recomputed in the test; the imposed sheet renders as the sources placed by those matrices; `tpdf impose` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-10

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-09 — Page geometry

**Page geometry**: set boxes over page ranges, scale to a paper size, resize and zoom, add margins or bleed, flatten `/Rotate` into content, normalise box origins, read `/UserUnit` everywhere a placement is measured, flatten inherited page attributes

**Today.** boxes settable; `/UserUnit` (PDF 1.6) is not read anywhere, so placements are measured in 1/72 inch — clause (3) of the image-recompression row before 9 October 2026; `features/cbz.md:432` names it as one way an oversized picture could be kept inside Annex C.2's limit (FI-26)

**Exit criterion.** fixtures per operation with renders equal to the twin built directly; a `/UserUnit` page's placements measured in its unit

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-10 — Overlay and underlay with page maps

**Overlay and underlay with page maps**: pages of one document on or under another's, by from/to/repeat maps, as forms, optionally in a layer

**Today.** `import_page_as_form` and `stamp` (`features/editing.md:242-273`), one page at a time

**Exit criterion.** a map applied over generated documents places the expected page on each target, checked by structure and render; `tpdf overlay` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-11 — One page-range grammar

**One page-range grammar**: ranges, last page, counting from the end, exclusions, odd and even, collation of interleaved duplex scans

**Today.** `--pages` ranges in the CLI

**Exit criterion.** the grammar's every form held to a table of inputs and page lists

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-12 — Split strategies

**Split strategies**: by page count, by top-level outline items (named after titles), by output size, by blank separator pages, by barcode separator pages

**Today.** split by ranges (`tpdf split`)

**Exit criterion.** each strategy over generated documents yields the expected pieces; blank detection uses RC-17

**Doc:** — · **Depends on:** ED-50 (barcodes)

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-13 — Merge fidelity

**Merge fidelity**: AcroForms merged with a renaming policy (with `/DR` and `/CO`), structure trees merged (parent tree renumbered, role maps, class maps, ID trees, namespaces), outlines per file or merged, named destinations renamed on conflict, page labels, optional content, attachments, output intents, JavaScript name trees, viewer preferences, links retargeted

**Today.** `import_page`; `merge` deduplicates

**Exit criterion.** a merged document of two forms fills both; a merged tagged document passes the PDF/UA checks each source passed; links land on the right pages

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-14 — Bates numbering

**Bates numbering** across an ordered set of files, with prefix, suffix, digits, placement and a range report, removable

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'bates' crates tools` finds nothing

**Exit criterion.** numbers continuous across files; the report matches; removal restores the render; `tpdf bates` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-15

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-15 — Headers, footers and page numbers

**Headers, footers and page numbers** from tokens (page n of N, dates, file name, Bates), per range and odd/even, shaped and embedded, written as `/Artifact /Pagination` so PDF/UA stays valid, recorded so they can be updated or removed

**Today.** `stamp` places one form

**Exit criterion.** outputs pass the PDF/UA artifact rules; update and removal restore the original render; `tpdf header-footer` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-45

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-16 — Watermarks and backgrounds that can be updated or removed

**Watermarks and backgrounds that can be updated or removed**: text, image or page sources, rotation, opacity, front or behind, as a `/Watermark` annotation with `/FixedPrint` or as layered content

**Today.** `stamp`

**Exit criterion.** removal restores the original render; print/view usage honoured (RC-13); `tpdf watermark` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-15

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-17 — Outline styling and document settings

**Outline styling and document settings**: outline item colour and style (`/C`, `/F`) read and written; outline import and export as JSON and text; outlines generated from structure headings or font-size heuristics; `/Lang`, `/PageMode`, `/PageLayout` and `/MarkInfo` set on an existing document

**Today.** outline read and written without `/C` or `/F` (`crates/tinker-pdf-cos/src/outline.rs` reads neither); `/Lang` set by the builder only

**Exit criterion.** fixtures for each key read and written; a generated outline matches the tagged headings of the committed books; `tpdf bookmarks`, `tpdf labels` and `tpdf metadata`, get and set runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-18 — A table-of-contents page

**A table-of-contents page** with leaders, page numbers and links generated from the outline

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'table_of_contents|toc_page' crates tools` finds nothing

**Exit criterion.** the generated page's links land on their targets; text extracts as the outline

**Doc:** — · **Depends on:** ED-45

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-19 — Article threads and page transitions

**Article threads and page transitions**: `/Threads` and beads read, followed and written; `/Trans` and `/Dur` read and written, and transition frames generated for a presenting host

**Today.** absent — the owner's parity decision of 9 October 2026; `/Threads` is read nowhere, and `/Trans` is named only in `sanitise`'s list of the actions that stay (`crates/tinker-pdf-cos/src/edit/sanitise.rs:86, 210`)

**Exit criterion.** fixtures read back to what was written; transition frames equal the 12.4.4 effect computed in the test

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-20 — Embedded page thumbnails

**Embedded page thumbnails**: read `/Thumb`, generate, embed, remove

**Today.** `/Thumb` appears only in the linearization rules

**Exit criterion.** a generated thumbnail equals the page rendered at the thumbnail scale

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-21 — Portfolios

**Portfolios**: the `/Collection` schema, item metadata, folders, sort, initial document and view mode read, created, edited and extracted

**Today.** attachments listed and written; `/Collection` not read

**Exit criterion.** a portfolio built and re-read yields its schema, folders and files byte for byte

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-22 — Document comparison, text

**Document comparison, text**: a word-level diff across documents or revisions with insert, delete, replace, format and move classes, as a report and as annotations

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\btext_diff\b|compare_documents|\bdocument_diff\b' crates tools` finds nothing

**Exit criterion.** generated document pairs with known edits yield exactly those edits; `tpdf compare --text` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-23 — Document comparison, visual

**Document comparison, visual**: pixel and vector overlay diff with colour coding, thresholds, shifted-page alignment and ignored regions

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'visual_diff|pixel_diff|compare_renders' crates tools` finds nothing

**Exit criterion.** generated pairs with known differences report exactly those regions; identical renders report none; `tpdf compare --visual` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-24 — Optimisation presets and font unification

**Optimisation presets and font unification**: screen, ebook, print and prepress presets; identical-but-differently-subset fonts across merged documents unified; embedded standard-14 programs dropped on request; content minified (number precision, operator coalescing)

**Today.** subsetting on rewrite by default, stream deduplication, image recoding (`SaveOptions::images`)

**Exit criterion.** each preset's output validates and renders within its stated tolerance; unification leaves one program per face; savings recorded on the committed fixtures; `tpdf optimise` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** FT-11

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-25 — Image recompression remainder

**Image recompression remainder**: (1) images stored through an image codec recoded, once the read side hands their samples in their own colour space; (2) images drawn only by a pattern cell, a Type 3 glyph or a soft mask resampled; average and bicubic resampling and a threshold resolution beside the box filter

**Today.** **Landed for images stored through the general filters** — `tinker_pdf::write::SaveOptions::images`, off by default, recodes per kind and box-filters to a caller's `max_ppi`, held by `crates/tinker-pdf/tests/image_recode.rs` and surfaced by `tpdf`'s writers as `--images`, `--bilevel` and `--max-ppi` ([features/writing.md](../features/writing.md)). **What is left, by name.** (1) An image stored as `DCTDecode`, `JPXDecode`, `CCITTFaxDecode` or `JBIG2Decode` is `UntouchedImageReason::Filter` (`features/writing.md:353`) — its samples *in their own colour space* are the read side's type, and recoding them waits on that rather than on a second decoder here; a photograph is commonly stored as DCT, so this is the part a downsampling caller is likeliest to meet. (2) An image only a tiling pattern's cell, a Type 3 glyph or a soft mask's group draws is recoded but never resampled (`Unplaced`): the walk interprets pages, forms and appearances only. (Clause (3), `/UserUnit`, is ED-09's.) No corpus measurement was taken: the fetched corpora were not in the container where the half landed

**Exit criterion.** as the landed half: lossless codings return the samples written, the page draws the same pixels, JPEG within the bound computed from the caller's table; resampling equals the filter computed in the test; the corpus measurement taken where the corpus is

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-26 — A JPEG 2000 encoder

**A JPEG 2000 encoder** (reversible and irreversible, tiles, layers, progression orders)

**Today.** absent — the owner's parity decision of 9 October 2026; `tinker-pdf-filters` writes PNG, CCITT G4, JBIG2 generic regions and baseline JPEG and no JPEG 2000 (`rg -i 'jpx_encode|encode_jpx' crates tools` finds nothing)

**Exit criterion.** T.800 Annex J.10's published image encodes to J.10's published codestream with J.10's parameters; every other configuration decodes through the in-tree decoder within the bound computed in the test (lossless exactly)

**Doc:** `design/jpx-encoder.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-27 — JBIG2 symbol and text-region encoding

**JBIG2 symbol and text-region encoding** (lossless)

**Today.** generic regions only; symbol and text encoding a non-goal of the design until 9 October 2026 (`design/jbig2-symbol-text.md:84-99`)

**Exit criterion.** every output decodes to the input exactly through the in-tree decoder; and T.88 Annex H.1's symbol dictionary and text-region segments reproduced byte for byte by re-encoding, under the same parameters, the symbols and placements they decode to — whose composite page H.1 publishes, though it publishes no symbol bitmap of its own (the design doc's milestone 3 says so)

**Doc:** [design/jbig2-symbol-text.md](../design/jbig2-symbol-text.md) (amend) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-28 — JPEG encoder breadth

**JPEG encoder breadth**: optimised Huffman tables (Annex K.2), progressive mode, documented table presets for callers who want a named quality

**Today.** `tinker_pdf_filters::jpeg_encode` writes T.81's **baseline** process and only that — sequential DCT, Huffman, 8-bit, one SOF0 frame, one scan; grayscale or T.871's YCbCr with the JFIF APP0, 4:4:4 or 4:2:0 named by the caller and never inferred from the pixels; Annex K's tables, those halved, or the caller's own — with **no 1-to-100 quality number**, because every such scale in circulation is some program's private convention and none is in T.81, T.83 or T.871. Progressive and extended sequential, arithmetic, lossless, hierarchical, 12-bit, four-component CMYK/YCCK and K.2's optimised tables are excluded by name in the module header. The FDCT is held to A.3.3's equation recomputed in `f64` in the test, because no published DCT vector set is reachable (H-98)

**Exit criterion.** K.2's procedure recomputed in the test yields the emitted tables; progressive output decodes to the baseline output's samples exactly

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-29 — Undo and redo

**Undo and redo**: a journal of named operations with undo, redo and inspection, a snapshot save, and the journal saved beside the file and reloaded when it matches

**Today.** transactions and checkpoints (`features/editing.md:274`)

**Exit criterion.** every editor operation undone restores the byte-identical previous save; a journal round-trips

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-30 — The low-level object API

**The low-level object API**: get, set and remove by path (`/Root/AcroForm/NeedAppearances`), deep copy and compare, a graft map that keeps sharing when copying objects between documents

**Today.** object-level get, put and allocate (`features/editing.md:11-88`)

**Exit criterion.** path operations held to fixtures; grafting two pages that share a font copies the font once

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-31 — Revisions

**Revisions**: count incremental revisions with byte ranges, open as of revision N, extract revision N, diff two revisions by value, roll back

**Today.** incremental update with a byte-identical prefix; signatures' `modifications()` compares xref entries

**Exit criterion.** fixtures with known revisions; extracted revision N is byte-equal to its prefix; the value diff lists exactly the changed objects

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-32 — Check and repair on demand

**Check and repair on demand**: force a repair, a throw-on-repair mode so an edit is never lost to a late repair, a structure-tree check and repair (parents, cycles, spans, table attributes)

**Today.** the leniency ladder repairs during open with typed warnings

**Exit criterion.** a damaged fixture forced through repair before editing keeps every edit; structure-tree repairs held to fixtures with known defects; `tpdf repair` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-33 — External streams internalised

**External streams internalised**: a stream with `/F` read through a host resource provider and written inside the file

**Today.** refused as `UntouchedImageReason::Filter` for an external `/F` (`features/writing.md:353`)

**Exit criterion.** an external-stream fixture saved self-contained renders identically

**Doc:** — · **Depends on:** FI-27

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-34 — References to removed pages dropped on save

**References to removed pages dropped on save** (in `tinker-pdf-cos`)

**Today.** `keep_pages` keeps the catalog, and a garbage-collecting rewrite follows it, so a page an outline item, a named destination or a link still names stays in a split piece outside its page tree — page 6 of `outline-3level.pdf` split out carries five page objects (`a_page_the_outline_names_stays_in_a_piece_that_dropped_it`; `features/editing.md:848`); `delete_page` leaves the same references, and structure and `/OpenAction` too. `tpdf split` says it is not a redaction. A limit of the facade's rather than the CLI's, found when the CLI landed

**Exit criterion.** a page dropped by `keep_pages` or `delete_page` no longer reachable from anything the saved document keeps: references to it nulled or pruned on save, the pinned test inverted

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-35 — Type 3 glyph procedures rewritten under redaction

**Type 3 glyph procedures rewritten under redaction** (redaction clause (a))

**Today.** Redaction cuts a Type 3 run in its own glyph space and a glyph at the use whose procedure draws under a rectangle, measured in the enclosing scope and in the font's own `/Resources` (`glyph_procedures`). **Glyph-procedure streams are not rewritten** (`features/editing.md:836`): the exit asked for them rewritten; what was built instead removes the *use* and leaves the procedure, which `subset::apply` — the default save — empties once nothing shown runs it (`subset.rs`'s `type3_fonts`, which walks tiling cells and mask groups and counts a glyph whose procedure this engine could not run), so a procedure some uncovered use still runs keeps the covered words in `/CharProcs`; and what a procedure draws — a form it invokes, a soft mask's group it sets — is not cut either, and emptying the procedure removes neither from the file (a group is measured at every use, one the glyph's own box removed included, and named `PatternOrMask` when it shows text under a rectangle there, whatever removed the use, `a_glyph_whose_procedure_masks_text_under_a_rectangle_is_removed` and `a_covered_glyph_whose_procedure_masks_text_under_a_rectangle_names_the_group` — or when the use's `MAX_PLACEMENTS`-stream budget cut its measurement short, `a_group_after_a_naming_that_spent_the_budget_is_named`, what the budget cut short anywhere else naming the font `UnboundedProcedure`, `a_covered_use_whose_measurement_ran_out_says_so`; a form is not named). That substitute was the implementing lane's choice, argued in `cut_stream`'s doc and the refusal table, and is the owner's to accept or refuse (H-11); until then this criterion is open

**Exit criterion.** glyph-procedure streams rewritten, so the covered words are absent from every procedure stream an uncovered use still runs, and what a procedure draws under a rectangle is cut; or the owner's ruling that removal at the use with `subset::apply`'s emptying suffices

**Doc:** — · **Depends on:** H-11

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-36a — Image redaction modes

**Image redaction modes**: none; the whole image removed; the pixels under a rectangle blanked and the image recoded losslessly; and removed unless invisible, by a clip-aware estimate of whether the covered part is drawn

**Today.** the whole image is scrubbed (`features/editing.md:822`). The image encoders exist — `tinker-pdf-filters` writes PNG, CCITT G4 and JBIG2 generic regions (15 September 2026) and baseline JPEG (16 September), and the writer calls them only when a save's `SaveOptions::images` names one — so this waits on the decision of when to choose a codec rather than on a codec (H-20)

**Exit criterion.** the blanked region's samples are the fill and every other sample is unchanged; each mode's survivors match fixtures with known clips; `tpdf redact --images <mode>` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** H-20

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-36b — Redaction modes for line art and text

**Redaction modes for line art and text**: line art none, remove if covered, remove if touched; text none (marks only) beside the default removal

**Today.** absent — the owner's parity decision of 9 October 2026; redaction removes text and images and leaves line art: the redaction module "does not follow paths" (`features/editing.md:824`)

**Exit criterion.** each mode held to fixtures with known survivors; `tpdf redact --line-art <mode>` and `--text none` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-36c — A flatten-to-image redaction mode

**A flatten-to-image redaction mode** that leaves no vector or text residue

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'flatten_to_image' crates tools` finds nothing

**Exit criterion.** the output page carries one image and no text, and its render equals the redacted render; `tpdf redact --flatten` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-36d — Mark, review, apply

**Mark, review, apply**: `/Redact` annotations created, listed and applied, with overlay text, codes, `/Repeat`, fill and cross-out

**Today.** redaction applies caller rectangles directly

**Exit criterion.** marks round-trip; application equals direct application with the same rectangles plus the overlay; `tpdf redact` by area and by search, marking and applying runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** AN-06

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-36e — No residue outside the content

**No residue outside the content**: structure elements, `/ActualText`, an annotation's own `/Contents`, field values, outline titles, named destinations and attachments that reference redacted content scrubbed; links and annotations in the area removed

**Today.** an annotation's own text is left when its appearance is cut (`features/editing.md:838`)

**Exit criterion.** fixtures with redacted words planted in each place find none after application (text search and raw-stream search); `tpdf redact`, by default runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-36f — Remove only invisible text

**Remove only invisible text** (render mode 3), the redaction mode for stripping an old OCR layer

**Today.** absent — the owner's parity decision of 9 October 2026; invisible text is shown to the device like any other (`invisible_text_is_still_shown_to_the_device`, `crates/tinker-pdf-content/src/interpret.rs:2378`) and no redaction mode keys on it

**Exit criterion.** fixtures with visible and invisible runs keep exactly the visible; `tpdf redact --invisible-text` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-37 — Write-option parity

**Write-option parity**: decompress everything, recompress already-filtered streams (Flate at a higher effort, Brotli), compression effort, compress only fonts or only images, ASCII-hex armour, pretty and compact object syntax, keep metadata untouched, continue on error, regenerate or keep `/ID`

**Today.** a filtered stream is "handed through untouched" (`features/writing.md:344`); `WriteOptions` carries mode, linearize, version, object streams, compress, encryption, garbage collection, deduplication

**Exit criterion.** each option held to fixtures; outputs validate strictly and render identically; `tpdf linearize` and `tpdf clean` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** FO-18

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-38 — Streaming write

**Streaming write**: finished pages flushed to the sink; memory proportional to the largest object

**Today.** a non-goal of the streaming design until 9 October 2026 (`design/streaming-open.md:47-66`)

**Exit criterion.** a 10 000-page generated document written with peak memory within an MS-05 band independent of page count

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-39 — Attachment parity

**Attachment parity**: checksum verification, update, delete, extraction from the CLI, file-attachment annotations embedded and extracted

**Today.** attachments listed and written; `tpdf attach` adds one

**Exit criterion.** each operation round-trips; a corrupted checksum is reported by a typed warning; `tpdf attachments list`, `add`, `remove` and `extract` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-40 — Layer authoring breadth

**Layer authoring breadth**: OCMDs with `/VE`, `/Configs`, the UI `/Order`, `/RBGroups`, optional content on annotations, images and existing content

**Today.** `add_layer`, `PageBuilder::optional`, visibility toggled in `/D`

**Exit criterion.** each authored structure re-read and rendered under each configuration as RC-14 decides; `tpdf layers` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** RC-14

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-41 — Images to PDF without re-encoding

**Images to PDF without re-encoding**: JPEG, JPEG 2000, CCITT and JBIG2 passed through; PNG through Flate with predictors (16-bit, palette, alpha to soft mask); multi-page TIFF to pages; resolution to page size; EXIF orientation; PDF/A output

**Today.** the builder deflates every raster it is handed (`features/creation.md:382`); JPEG and PNG IDAT pass through

**Exit criterion.** the image stream bytes equal the input's coded bytes; decoded samples equal the source's

**Doc:** — · **Depends on:** FI-14

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-42 — Convert any opened document to PDF and insert its pages

**Convert any opened document to PDF and insert its pages**: EPUB, XPS, CBZ, SVG, HTML, Markdown, FB2 and images inserted into an existing PDF with links

**Today.** each format is synthesised into a real PDF on open; nothing inserts one into another

**Exit criterion.** inserted pages render as the opened document's pages; links retargeted; `tpdf convert --insert` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-13

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-43 — HTML into a rectangle of an existing page

**HTML into a rectangle of an existing page**, with scaling to fit, rotation, layer and opacity, a positions callback and overflow reporting; a DOM API to change the HTML before layout — create, find, insert, remove and clone elements and text, and set attributes

**Today.** `FromHtml` makes whole documents (`features/creation.md:13-45`)

**Exit criterion.** the placed content renders as the same HTML laid out at that size; overflow reported exactly when content exceeds the box; a DOM edit lays out as the same HTML written with the edit

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-44 — A programmatic layout API

**A programmatic layout API**: paragraph, division, table, list, image and break with pagination, repeating table headers and footers, row spans, keep-together and keep-with-next, floats, columns, justification and hyphenation

**Today.** "layout on the builder's own methods — by design" (`features/creation.md:385`)

**Exit criterion.** layouts built through the API equal the same content laid out from HTML (reftest pairs); a 10 000-row table streams within an MS-05 band

**Doc:** `design/layout-api.md` (owed) · **Depends on:** FT-15

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-45 — Shaped text on the builder

**Shaped text on the builder**: text in any font and script shaped, embedded and subset in one call; simple fonts for Latin, Greek and Cyrillic encodings; a non-embedded CJK font with a predefined CMap; a text box filled with alignment and overflow reporting

**Today.** `text` is one byte per character; shaping is `glyph_run`'s (`features/creation.md:383`)

**Exit criterion.** extracted text equals the input in logical order for every committed shaping fixture's script; overflow reported exactly

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-46 — Drawing helpers

**Drawing helpers**: lines, polylines, rectangles, circles, ellipses, sectors, Bézier curves, zigzags, with stroke and fill options

**Today.** `fill_rect` and raw content

**Exit criterion.** each helper's path equals the geometry computed in the test

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-47 — Links to non-ASCII targets

**Links to non-ASCII targets**: IRIs percent-encoded to URIs on write

**Today.** refused (`features/creation.md:384`; `PageBuilder::link` returns `false`)

**Exit criterion.** fixtures with non-ASCII targets write the RFC 3987 mapping and read back

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-48 — An SVG or another document's page placed as vector content

**An SVG or another document's page placed as vector content**: an SVG drawn as a form on a page or used as an image source in the builder

**Today.** SVG opens as its own document only

**Exit criterion.** the placed SVG renders as the opened SVG at that size

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-49 — Vector barcodes

**Vector barcodes**: Code 128 and GS1-128, Code 39 and 93, EAN-13 and 8, UPC-A and E, ITF-14, Codabar, MSI, postal codes, QR (all versions and error-correction levels, Kanji, structured append), Micro QR, rMQR, Data Matrix (ECC 200 and GS1), PDF417 and MicroPDF417, Aztec, MaxiCode, DataBar family, payment QR profiles; quiet zones and human-readable text

**Today.** absent — the owner's parity decision of 9 October 2026; "barcode" appears in two doc comments and no code (`rg -i 'barcode' crates`)

**Exit criterion.** each symbology held to its ISO/IEC specification's worked examples transcribed into tests; the decoder (ED-50) reads every generated symbol back; `tpdf barcode` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-50 — Barcode decoding

**Barcode decoding** from pages and images, at any rotation

**Today.** absent — the owner's parity decision of 9 October 2026; no symbology is decoded (ED-49's search)

**Exit criterion.** every symbol generated by ED-49 at every rotation, scale and quiet zone decodes to its input; published worked examples decode; `tpdf barcode --read` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-49

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## ED-51 — Create a document from annotated content-stream text

**Create a document from annotated content-stream text** (media box, rotation, fonts by file or standard name, CJK fonts, images by comment)

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'from_content_text|annotated_content' crates tools` finds nothing

**Exit criterion.** the created document renders as the same content built through the builder; `tpdf create` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*
