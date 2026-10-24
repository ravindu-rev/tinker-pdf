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

**The tree at `5297133`, against this row's text:**

- Ruling 7 (`docs/rulings.md:197-202`) excepts 'the `ContentFilter` rewrite path built for redaction ... which is itself part of the interpreter', but no `ContentFilter` exists (`rg ContentFilter crates` finds nothing): redaction tokenizes on its own in the facade (`crates/tinker-pdf/src/redact.rs:560`, `:4494`), as do `crates/tinker-pdf/src/subset.rs:765` and `crates/tinker-pdf/src/pdfa/content.rs:222`. ED-01 reads operators through the interpreter instead (ED-01.4), so it needs no further exception.
- The redaction rewrite path the Today cell names is lossy for operators it does not cut: every operand is re-emitted through `write_token` (`crates/tinker-pdf/src/redact.rs:4730-4745`, `:5085`), which writes a name's decoded bytes with no `#xx` escape, and `escape_into` (`:5076`) leaves a 0x0D byte raw inside parentheses, which the tokenizer reads back as 0x0A (`crates/tinker-pdf-content/src/tokenizer.rs:242-248`); `apply` writes the result back whatever was cut (`redact.rs:1214`). ED-01.3 fixes it.
- `docs/features/editing.md:98` says `append_content` appends operators 'to a page's content array'; `crates/tinker-pdf-cos/src/edit/page_ops.rs:346-373` instead decodes and joins the existing parts and writes one new stream (`q`, the old content, `Q q`, the operators, `Q`) that replaces `/Contents`, so the page's original streams are left unreferenced rather than byte-stable.
- The tree already enumerates one object kind read-only: `Page::images` (`crates/tinker-pdf/src/images.rs:328`) lists a page's image XObjects and inline images with a transform per drawing (`PageImage::placements`) through the interpreter; the Today cell names only the two writing paths. ED-01.8 holds the model's image objects to it.

## ED-02 — In-place text editing with reflow

**In-place text editing with reflow**: edit a line or paragraph of existing text, reshape with the subset font where its glyphs exist, extend the subset or fall back otherwise, reflow inside the paragraph box, keep tags and `/ActualText`

**Today.** absent — the owner's parity decision of 9 October 2026; content is changed by `append_content` and redaction's rewrite only (ED-01); `rg -i 'edit_text|replace_text' crates tools` finds nothing

**Exit criterion.** an edited paragraph extracts as the new text, draws within its box, keeps its structure element, and its font subset gains exactly the new glyphs; fixtures per script direction

**Doc:** `design/text-editing.md` (owed) · **Depends on:** ED-01, FT-11, FT-30

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-02.1 | design | Create `docs/design/text-editing.md`: what a paragraph is (a tagged `P`, or lines FT-30's segmentation groups), its box and alignment read from line edges and leading, how new text is shaped, how missing glyphs are found (the subset, an extended subset through FT-11 with a matching full face, then a fallback chain), how lines are broken and justified, how marked content and `/ActualText` survive, what happens on overflow, and each script direction's rules. | `docs/design/text-editing.md` (new) | The doc exists in the design-doc order (scope, non-goals, design, milestones, risks), names ED-02.2 to ED-02.16's exit tests, and is reviewed. | ED-01 |
| ED-02.2 | build | Paragraphs from tagged pages: a `P` element's marked content gathered from ED-01's text runs into a paragraph with its lines, runs, fonts, MCIDs and `/ActualText` spans. | `crates/tinker-pdf/src/page_objects/paragraph.rs` (new), `crates/tinker-pdf/src/page_objects/mod.rs` (new), `crates/tinker-pdf/tests/text_editing.rs` (new) | `text_editing.rs` (new) `a_tagged_paragraph_is_read_with_its_lines`: builder paragraphs tagged `P` read back with the lines, runs and MCIDs the builder wrote. | ED-02.1 |
| ED-02.3 | build | Paragraphs from untagged pages: lines grouped into a paragraph by FT-30's region segmentation and reading order. | `crates/tinker-pdf/src/page_objects/paragraph.rs` (new), `crates/tinker-pdf/tests/text_editing.rs` (new) | `an_untagged_paragraph_is_grouped`: builder pages of several paragraphs and two columns group into the paragraphs the builder drew, none spanning a column. | ED-02.2, FT-30 |
| ED-02.4 | build | A paragraph's box, leading, word spacing and alignment (left, right, centred, justified) read from its lines' edges and baselines. | `crates/tinker-pdf/src/page_objects/paragraph.rs` (new), `crates/tinker-pdf/tests/text_editing.rs` (new) | `box_and_alignment_are_read`: for builder paragraphs of each alignment, the box, leading and alignment equal the values the test computes from the builder's input. | ED-02.2 |
| ED-02.5 | build | New text shaped with the paragraph's font through the shaper, returning glyphs the subset holds and the list of characters it does not. | `crates/tinker-pdf/src/page_objects/paragraph.rs` (new), `crates/tinker-pdf/src/shaping.rs`, `crates/tinker-pdf/tests/text_editing.rs` (new) | `new_text_is_shaped_with_the_subset`: Latin, Arabic and Devanagari replacements shape to the glyphs the test expects where the subset holds them, and report exactly the missing characters where it does not. | ED-02.2 |
| ED-02.6 | build | The subset extended: missing glyphs taken from a matching full face the host supplies, through FT-11's subset extension, and the embedded program rewritten. | `crates/tinker-pdf/src/page_objects/paragraph.rs` (new), `crates/tinker-pdf/src/subset.rs`, `crates/tinker-pdf/tests/text_editing.rs` (new) | `the_subset_gains_exactly_the_new_glyphs`: after an edit needing three new glyphs, the embedded subset holds the old glyphs plus exactly those three, and the page renders and extracts the new text. | ED-02.5, FT-11 |
| ED-02.7 | build | Fallback: characters no supplied face holds drawn in the engine's fallback faces, added to the page's resources as their own runs. | `crates/tinker-pdf/src/page_objects/paragraph.rs` (new), `crates/tinker-pdf/tests/text_editing.rs` (new) | `missing_glyphs_fall_back`: a replacement with characters the subset and no host face hold extracts as the new text in both builds, the fallback runs named by a typed warning. | ED-02.6 |
| ED-02.8 | build | Reflow: the shaped text broken into lines inside the paragraph's box by the layout crate's line breaking, at the paragraph's leading and alignment, left-to-right scripts first. | `crates/tinker-pdf/src/page_objects/paragraph.rs` (new), `crates/tinker-pdf/tests/text_editing.rs` (new) | `a_reflowed_paragraph_stays_in_its_box`: longer and shorter replacements break into lines whose ink lies inside the box, at positions the test computes for each alignment. | ED-02.4, ED-02.5 |
| ED-02.9 | build | Justification: a justified paragraph's lines filled to the box by word spacing, the last line set as the paragraph's own last line was. | `crates/tinker-pdf/src/page_objects/paragraph.rs` (new), `crates/tinker-pdf/tests/text_editing.rs` (new) | `justified_lines_fill_the_box`: every line but the last ends at the box edge to the test's tolerance; the last keeps its alignment. | ED-02.8 |
| ED-02.10 | build | Write-back: the paragraph's old runs replaced by the new lines through ED-01's edits, each line's marked content keeping its MCID so the `P` still claims the text, and `/ActualText` spans carried or rewritten. | `crates/tinker-pdf/src/page_objects/paragraph.rs` (new), `crates/tinker-pdf/src/page_objects/write.rs` (new), `crates/tinker-pdf/tests/text_editing.rs` (new) | `an_edited_paragraph_keeps_its_structure`: the edited paragraph extracts as the new text, its structure element's text is the new text, and every other element's text is unchanged. | ED-02.7, ED-02.9 |
| ED-02.11 | build | Right-to-left and mixed paragraphs: Arabic and Hebrew text shaped, joined and laid out in visual order per line, numbers and Latin embedded by the bidi algorithm. | `crates/tinker-pdf/src/page_objects/paragraph.rs` (new), `crates/tinker-pdf/tests/text_editing.rs` (new) | `right_to_left_paragraphs_edit`: Arabic and Hebrew paragraphs with embedded numbers and Latin extract in logical order as the new text and draw inside the box, in both builds. | ED-02.10 |
| ED-02.12 | build | Vertical paragraphs: text in a vertical font (`Identity-V`) reflowed top to bottom in columns from right to left. | `crates/tinker-pdf/src/page_objects/paragraph.rs` (new), `crates/tinker-pdf/tests/text_editing.rs` (new) | `vertical_paragraphs_edit`: a vertical Japanese paragraph extracts as the new text and draws inside its box. | ED-02.10 |
| ED-02.13 | build | Overflow: text that cannot fit the box is refused with a typed error naming the lines that would not fit, leaving the page unchanged. | `crates/tinker-pdf/src/page_objects/paragraph.rs` (new), `crates/tinker-pdf/tests/text_editing.rs` (new) | `overflow_is_refused_and_changes_nothing`: an over-long replacement returns the error and the saved page is byte-identical to the original. | ED-02.10 |
| ED-02.14 | build | The surface: `DocumentEditor` edits a paragraph by index or by the text it holds, and the feature doc describes the rules and the refusals. | `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/src/page_objects/mod.rs` (new), `docs/features/editing.md` | The facade tests reach every path (tagged, untagged, extended, fallback, overflow) through the public API. | ED-02.11, ED-02.12, ED-02.13 |
| ED-02.15 | test | A fuzz target editing paragraphs on hostile pages: arbitrary content over fixed resources, an edit applied, and the result re-opened. | `fuzz/fuzz_targets/paragraph_edit.rs` (new), `fuzz/Cargo.toml`, `fuzz/README.md`, `docs/verification.md` | The target builds and runs clean in the fuzz check, its seeds replay in `fuzz-seeds`, and the roster `docs/verification.md` pastes is regenerated so `cargo xtask check` passes. | ED-02.14 |
| ED-02.16 | measure | The editing operation's speed row and fingerprint: one paragraph edited on each of 100 generated pages, and one edit's saved bytes fingerprinted. | `crates/tinker-pdf/benches/engine.rs`, `crates/tinker-pdf/tests/determinism.rs` | The operation runs in the benchmark set with its figure recorded; the fingerprint holds on every K-07 target. | ED-02.14, MS-04, K-07 |

**Closes the row:** ED-02.2 to ED-02.13 edit a paragraph so it extracts as the new text, draws in its box, keeps its structure element and gains exactly the new glyphs, with fixtures per direction (ED-02.11, ED-02.12), which is the row's exit; ED-02.14 to ED-02.16 add the surface, fuzzing, speed row and fingerprint.

## ED-03 — Template text replacement

**Template text replacement**: placeholders in existing content replaced with font fallback and alignment

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'replace_text|template_text' crates tools` finds nothing

**Exit criterion.** fixtures with placeholders replaced and extracted back; alignment computed in the test

**Doc:** — · **Depends on:** ED-01

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-03.1 | build | Placeholders in existing content found by their text across ED-01's text runs and replaced in the run's font where its glyphs exist, through `DocumentEditor`. | `crates/tinker-pdf/src/page_objects/placeholders.rs` (new), `crates/tinker-pdf/src/page_objects/mod.rs` (new), `crates/tinker-pdf/tests/placeholders.rs` (new), `docs/features/editing.md` | `placeholders.rs` (new) `placeholders_are_replaced_and_read_back`: builder fixtures with placeholders, one split across two runs, extract with every value in place. | ED-01 |
| ED-03.2 | build | Alignment: a value placed at its placeholder's left edge, right edge or centre, its width from the font's metrics. | `crates/tinker-pdf/src/page_objects/placeholders.rs` (new), `crates/tinker-pdf/tests/placeholders.rs` (new) | `replacements_align_as_asked`: each value's drawn extent equals the position the test computes from the font's widths for each alignment. | ED-03.1 |
| ED-03.3 | build | Font fallback: characters the placeholder's font lacks drawn in a fallback face added to the page's resources, and a fingerprint of one replaced page. | `crates/tinker-pdf/src/page_objects/placeholders.rs` (new), `crates/tinker-pdf/tests/placeholders.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | `replacements_fall_back_for_missing_glyphs`: a Greek and an Arabic value in a Latin subset extract back in both builds; the fingerprint holds on every K-07 target. | ED-03.2, K-07 |

**Closes the row:** ED-03.1 to ED-03.3 replace placeholders with fallback and alignment, extracted back and aligned as the test computes, which is the row's exit.

## ED-04 — Content-stream cleaning

**Content-stream cleaning**: normalise and pretty-print operators, balance `q`/`Q`, remove top-level state leaks and redundant colour operators, keep marked content; the content-filter pipeline public on the facade

**Today.** the redaction rewrite path (`redact.rs`'s `cut_stream`, ruling 7) is internal

**Exit criterion.** a cleaned page renders byte-identically to the original on every fingerprint fixture and the corpus subset re-run; `q`/`Q` balanced by count

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-04.1 | build | The content-filter pipeline public on the facade: a filter over a page's operators built on ED-01's spans, writer and splice, with redaction's `cut_stream` moved onto it. | `crates/tinker-pdf-content/src/filter.rs` (new), `crates/tinker-pdf-content/src/lib.rs`, `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/src/lib.rs`, `docs/features/editing.md` | Every redaction test passes unchanged on the public pipeline; a no-op filter returns every fingerprint fixture's content byte-identical. | ED-01 |
| ED-04.2 | build | Normalise and pretty-print: one operator per line, numbers in their shortest form, names and strings through ED-01's writer. | `crates/tinker-pdf-content/src/filter.rs` (new), `crates/tinker-pdf/tests/content_clean.rs` (new) | `content_clean.rs` (new) `a_normalised_page_renders_identically`: every fingerprint fixture renders byte-identically after normalising, and normalising twice changes nothing. | ED-04.1 |
| ED-04.3 | build | `q` and `Q` balanced: a missing `Q` added at the end and a `Q` with no `q` dropped, as 8.4.2 and the interpreter already treat them. | `crates/tinker-pdf-content/src/filter.rs` (new), `crates/tinker-pdf/tests/content_clean.rs` (new) | `q_and_q_balance_by_count`: fixtures with leaks and stray restores balance by count and render byte-identically before and after. | ED-04.2 |
| ED-04.4 | build | Top-level state leaks and redundant colour operators removed, marked content kept: a colour set again with no paint between dropped, and state left set at the end of the stream wrapped so content appended later starts clean. | `crates/tinker-pdf-content/src/filter.rs` (new), `crates/tinker-pdf/tests/content_clean.rs` (new) | `leaks_and_redundant_colours_go_and_marked_content_stays`: fixtures render byte-identically, lose the redundant operators the test counts, and keep every `BDC`/`EMC` and MCID so the structure tree's text is unchanged. | ED-04.3 |
| ED-04.5 | test | The corpus subset re-run: a census in `corpus.yml` that cleans every page of a pinned subset and compares its render with the original's. | `crates/tinker-pdf/tests/content_clean_census.rs` (new), `.github/workflows/corpus.yml` | The census prints `RAN` and asserts every page of the subset renders byte-identically after cleaning, on a dispatched run on `develop`. | ED-04.4, MS-11, H-03 |
| ED-04.6 | build | `tpdf clean`, the speed row for cleaning 100 generated pages and a fingerprint of one cleaned page. | `tools/tpdf/src/main.rs`, `crates/tinker-pdf/benches/engine.rs`, `crates/tinker-pdf/tests/determinism.rs`, `docs/features/editing.md` | A CLI test cleans a fixture to the facade's bytes; the speed row is in the benchmark set; the fingerprint holds on every K-07 target. | ED-04.4, MS-04, K-07 |

**Closes the row:** ED-04.2 to ED-04.4 clean a page so it renders byte-identically with `q`/`Q` balanced, ED-04.5 re-runs a corpus subset, and ED-04.1 makes the pipeline public, which is the row's exit.

## ED-05 — Text to outlines

**Text to outlines**: convert all text to vector paths, Type 3 glyphs flattened

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'text_to_paths|text_to_outlines|outline_text' crates tools` finds nothing

**Exit criterion.** the converted page renders identically and extracts no text

**Doc:** — · **Depends on:** ED-04

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-05.1 | build | Text to outlines through ED-04's pipeline: each glyph of a text run emitted as its outline path in the text's matrix, filled or stroked as its render mode draws it, invisible text dropped. | `crates/tinker-pdf/src/outline_text.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/text_to_outlines.rs` (new), `docs/features/editing.md` | `text_to_outlines.rs` (new) `converted_pages_render_alike_and_hold_no_text`: TrueType, CFF and Type 1 fixtures in fill, stroke and fill-and-stroke modes render identically after conversion and extract no text, in both builds. | ED-04 |
| ED-05.2 | build | Clipping modes (4 to 7) converted to the clip path their glyphs accumulate until `ET`. | `crates/tinker-pdf/src/outline_text.rs` (new), `crates/tinker-pdf/tests/text_to_outlines.rs` (new) | `clipping_text_becomes_a_clip_path`: each clipping mode's fixture renders identically after conversion and extracts no text. | ED-05.1 |
| ED-05.3 | build | Type 3 glyphs flattened: each glyph's procedure inlined in its font and text matrices, `d0` and `d1` honoured. | `crates/tinker-pdf/src/outline_text.rs` (new), `crates/tinker-pdf/tests/text_to_outlines.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | `type_3_glyphs_flatten`: Type 3 fixtures, coloured and uncoloured, render identically after conversion and extract no text; a converted page is fingerprinted on every K-07 target. | ED-05.1, K-07 |

**Closes the row:** ED-05.1 to ED-05.3 convert every kind of text to paths so the page renders identically and extracts no text, which is the row's exit.

## ED-06a — Recolour a document to gray, RGB or CMYK

**Recolour a document to gray, RGB or CMYK** by device-space arithmetic, shadings resampled, output intents optionally removed

**Today.** absent — the owner's parity decision of 9 October 2026; colour is converted for rendering only (`rg -i 'convert_colou?rs|to_grayscale' crates tools` finds nothing)

**Exit criterion.** every colour operator, image and shading converted; the render equals the original render converted in the test (8.6.4's device-space formulas); `tpdf recolour` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-04

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-06a.1 | build | Colour operators converted to gray, RGB or CMYK by 8.6.4's device-space formulas through ED-04's pipeline, `g`/`rg`/`k` and their stroke twins and device `sc`/`scn` alike. | `crates/tinker-pdf/src/recolour.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/recolour.rs` (new), `docs/features/editing.md` | `recolour.rs` (new) `operators_convert_by_the_device_formulas`: each target's render equals the original render converted pixel by pixel in the test by 8.6.4's formulas. | ED-04 |
| ED-06a.2 | build | Images converted: device-space images and `Indexed` palettes converted by the same formulas and written back with Flate. | `crates/tinker-pdf/src/recolour.rs` (new), `crates/tinker-pdf/tests/recolour.rs` (new) | `images_convert_by_the_device_formulas`: each image's decoded samples equal the original's converted in the test, for 1-, 8- and 16-bit and indexed images. | ED-06a.1 |
| ED-06a.3 | build | Shadings converted: Type 2 and 3 functions under a linear conversion by converting their endpoints, every other shading resampled into a sampled function in the target space. *Note:* If a resampled shading's render differs from the converted original between samples, the run records the figures and stops for the owner (a price), never widens the comparison. | `crates/tinker-pdf/src/recolour.rs` (new), `crates/tinker-pdf/tests/recolour.rs` (new) | `shadings_convert`: linear-class shadings render equal to the original converted in the test; resampled shadings equal it at every sample point. | ED-06a.1 |
| ED-06a.4 | build | Output intents optionally removed, `tpdf recolour --to gray\|rgb\|cmyk`, and a fingerprint of one recoloured page. | `tools/tpdf/src/main.rs`, `crates/tinker-pdf/src/recolour.rs` (new), `crates/tinker-pdf/tests/recolour.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | `tpdf recolour` runs over every `testdata/` fixture to the facade's output; with the option the output intents are gone; the fingerprint holds on every K-07 target. | ED-06a.2, ED-06a.3, K-07 |

**Closes the row:** ED-06a.1 to ED-06a.3 convert every colour operator, image and shading by 8.6.4's formulas, checked against the converted render, and ED-06a.4 runs it from `tpdf recolour`, which is the row's exit.

## ED-06b — ICC-managed colour conversion of documents

**ICC-managed colour conversion of documents**: object classes to gray, sRGB, CMYK or the output intent with rendering intents, black-point compensation, device links, pure-black text kept, spots to process (with aliases), rich black, total-area-coverage reduction

**Today.** absent — the owner's parity decision of 9 October 2026; `tinker-pdf-color` converts for rendering only (`rg -i 'convert_document|output_intent_convert' crates tools` finds nothing)

**Exit criterion.** per conversion, the converted values equal the transform recomputed in the test; TAC under the cap on every pixel of the converted render; `tpdf recolour --profile` runs it on `testdata/` fixtures

**Doc:** `design/colour-conversion.md` (owed) · **Depends on:** RC-01, RC-02, RC-20

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-06b.1 | design | Create `docs/design/colour-conversion.md`: object classes and their selection, targets (gray, sRGB, CMYK, the output intent), rendering intents (RC-01), black-point compensation (RC-02), device links (RC-20), pure-black text, rich black, spots to process with an alias table, and the total-area-coverage reduction. | `docs/design/colour-conversion.md` (new) | The doc exists in the design-doc order, names ED-06b.2 to ED-06b.9's exit tests, and is reviewed. | RC-01, RC-02, RC-20 |
| ED-06b.2 | build | The conversion core: colour operators of each object class converted through an ICC transform built by the colour crate, with the intent and black-point compensation the class asks for. | `crates/tinker-pdf/src/recolour.rs` (new), `crates/tinker-pdf-color/src/icc.rs`, `crates/tinker-pdf/tests/recolour_icc.rs` (new) | `recolour_icc.rs` (new) `operators_equal_the_transform_recomputed`: converted values equal the transform recomputed in the test for each intent, with and without compensation. | ED-06b.1, ED-06a |
| ED-06b.3 | build | Images and shadings through the same transform, shadings resampled as ED-06a does. | `crates/tinker-pdf/src/recolour.rs` (new), `crates/tinker-pdf/tests/recolour_icc.rs` (new) | `images_and_shadings_equal_the_transform`: every sample equals the transform recomputed in the test. | ED-06b.2 |
| ED-06b.4 | build | Device links: a device-link profile, when given, used as the whole transform in place of the source and destination pair. | `crates/tinker-pdf/src/recolour.rs` (new), `crates/tinker-pdf/tests/recolour_icc.rs` (new) | `device_links_convert`: values equal the device link evaluated in the test. | ED-06b.2 |
| ED-06b.5 | build | Pure-black text kept and rich black handled: black text converted to the target's black alone, and rich black kept or reduced as the design states. | `crates/tinker-pdf/src/recolour.rs` (new), `crates/tinker-pdf/tests/recolour_icc.rs` (new) | `black_text_stays_pure`: black text in gray, RGB and CMYK fixtures converts to K alone in CMYK; rich black follows the design's rule. | ED-06b.2 |
| ED-06b.6 | build | Spots to process: Separation and DeviceN colours converted through their alternate and then the transform, with an alias table merging spot names. | `crates/tinker-pdf/src/recolour.rs` (new), `crates/tinker-pdf/tests/recolour_icc.rs` (new) | `spots_convert_to_process`: spot colours equal their tint transform composed with the ICC transform in the test; aliased spots convert as their target. | ED-06b.2 |
| ED-06b.7 | build | Total-area-coverage reduction: after conversion to CMYK, every colour above the cap reduced by the design's grey-component replacement. | `crates/tinker-pdf/src/recolour.rs` (new), `crates/tinker-pdf/tests/recolour_icc.rs` (new) | `coverage_stays_under_the_cap`: the converted render, rendered to CMYK, is at or under the cap on every pixel. | ED-06b.3, ED-06b.6 |
| ED-06b.8 | build | The output intent as a target, and `tpdf recolour --profile`. | `tools/tpdf/src/main.rs`, `crates/tinker-pdf/src/recolour.rs` (new), `docs/features/editing.md` | `tpdf recolour --profile` runs over every `testdata/` fixture to the facade's output; converting to the document's output intent equals the transform to that profile. | ED-06b.7 |
| ED-06b.9 | measure | The conversion's speed row and fingerprint: 100 generated pages converted to CMYK, and one converted page fingerprinted. | `crates/tinker-pdf/benches/engine.rs`, `crates/tinker-pdf/tests/determinism.rs` | The operation runs in the benchmark set with its figure recorded; the fingerprint holds on every K-07 target. | ED-06b.8, MS-04, K-07 |

**Closes the row:** ED-06b.2 to ED-06b.7 convert every object class with values equal to the transform recomputed and coverage under the cap, and ED-06b.8 runs it from `tpdf recolour --profile`, which is the row's exit.

## ED-07 — Clip, trim and auto-crop

**Clip, trim and auto-crop**: remove content outside a rectangle; keep or exclude relative to a box with margins; crop to content with a tolerance

**Today.** crop and other boundaries settable (`features/editing.md:89-106`); content is never removed

**Exit criterion.** removed objects absent from the stream; the visible render unchanged; auto-crop box equals the bounding box of marks (RC-17); `tpdf trim` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-04

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-07.1 | build | Remove content outside a rectangle: objects (ED-01) wholly outside it dropped through ED-04's pipeline, objects crossing it kept under a clip to it. | `crates/tinker-pdf/src/trim.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/trim.rs` (new), `docs/features/editing.md` | `trim.rs` (new) `outside_objects_go_and_the_render_stays`: removed objects are absent from the stream, and the render inside the rectangle is byte-identical to the original's region. | ED-04 |
| ED-07.2 | build | Keep or exclude relative to a page box with margins: the rectangle taken from the crop, trim or media box grown or shrunk by margins, and exclusion removing what lies inside instead. | `crates/tinker-pdf/src/trim.rs` (new), `crates/tinker-pdf/tests/trim.rs` (new) | `boxes_with_margins_keep_or_exclude`: fixtures per box and margin keep or exclude exactly the objects the test computes, the visible render unchanged. | ED-07.1 |
| ED-07.3 | build | Auto-crop: the crop box set to the bounding box of the page's marks (RC-17), grown by a tolerance. | `crates/tinker-pdf/src/trim.rs` (new), `crates/tinker-pdf/tests/trim.rs` (new) | `auto_crop_takes_the_marks_box`: the crop box equals RC-17's bounding box of marks plus the tolerance on builder pages with known marks. | ED-07.1, RC-17 |
| ED-07.4 | build | `tpdf trim` (rectangle, box with margins, auto) and a fingerprint of one trimmed page. | `tools/tpdf/src/writing.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/determinism.rs` | `tpdf trim` runs over every `testdata/` fixture to the facade's output; the fingerprint holds on every K-07 target. | ED-07.2, ED-07.3, K-07 |

**Closes the row:** ED-07.1 to ED-07.3 remove what lies outside, keep or exclude by box, and auto-crop to RC-17's box with the render unchanged, and ED-07.4 runs `tpdf trim`, which is the row's exit.

## ED-08 — Imposition

**Imposition**: N-up with order and borders, saddle-stitch booklet, grid, cut a page into tiles, poster with overlap and marks

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'imposition|booklet|\bn_up\b' crates tools` finds nothing

**Exit criterion.** each layout's placement matrices recomputed in the test; the imposed sheet renders as the sources placed by those matrices; `tpdf impose` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-10

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-08.1 | build | Placement matrices for every layout: N-up (2, 4, 6, 8, 9, 16) in row or column order, left to right or right to left, with gaps and borders; booklet order; grids; tiles; posters with overlap. | `crates/tinker-pdf/src/impose.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/impose.rs` (new) | `impose.rs` (new) `placement_matrices_are_recomputed`: each layout's matrices equal those the test recomputes from the sheet and source sizes. | ED-10 |
| ED-08.2 | build | N-up sheets: each source page placed as a form (ED-10) by its matrix, with borders drawn. | `crates/tinker-pdf/src/impose.rs` (new), `crates/tinker-pdf/tests/impose.rs` (new), `docs/features/editing.md` | `n_up_sheets_render_as_placed`: each sheet renders as the sources rendered and placed by the matrices in the test. | ED-08.1 |
| ED-08.3 | build | Saddle-stitch booklets: pages ordered for folded sheets, padded with blanks to a multiple of four, placed two up front and back. | `crates/tinker-pdf/src/impose.rs` (new), `crates/tinker-pdf/tests/impose.rs` (new) | `booklets_order_and_place`: page order equals the test's for 1 to 17 pages; sheets render as placed. | ED-08.2 |
| ED-08.4 | build | Grids and tiles: a page cut into tiles, each a page cropped to its cell. | `crates/tinker-pdf/src/impose.rs` (new), `crates/tinker-pdf/tests/impose.rs` (new) | `tiles_render_as_their_regions`: each tile renders byte-equal to the same region of the source page. | ED-08.1 |
| ED-08.5 | build | Posters: tiles with overlap, crop marks and registration marks in the margins. | `crates/tinker-pdf/src/impose.rs` (new), `crates/tinker-pdf/tests/impose.rs` (new) | `posters_overlap_and_mark`: neighbouring tiles share the asked overlap and the marks lie where the test computes them. | ED-08.4 |
| ED-08.6 | build | `tpdf impose`, a speed row for imposing 100 generated pages 4-up and a fingerprint of one sheet. | `tools/tpdf/src/writing.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/benches/engine.rs`, `crates/tinker-pdf/tests/determinism.rs` | `tpdf impose` runs each layout over `testdata/` fixtures to the facade's output; the speed row is in the benchmark set; the fingerprint holds on every K-07 target. | ED-08.3, ED-08.5, MS-04, K-07 |

**Closes the row:** ED-08.1 recomputes every layout's matrices and ED-08.2 to ED-08.5 render each layout as the sources placed by them, and ED-08.6 runs `tpdf impose`, which is the row's exit.

## ED-09 — Page geometry

**Page geometry**: set boxes over page ranges, scale to a paper size, resize and zoom, add margins or bleed, flatten `/Rotate` into content, normalise box origins, read `/UserUnit` everywhere a placement is measured, flatten inherited page attributes

**Today.** boxes settable; `/UserUnit` (PDF 1.6) is not read anywhere, so placements are measured in 1/72 inch — clause (3) of the image-recompression row before 9 October 2026; `features/cbz.md:432` names it as one way an oversized picture could be kept inside Annex C.2's limit (FI-26)

**Exit criterion.** fixtures per operation with renders equal to the twin built directly; a `/UserUnit` page's placements measured in its unit

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-09.1 | build | Boxes over page ranges, box origins normalised (content translated so the media box starts at the origin), and inherited page attributes written onto each page. | `crates/tinker-pdf-cos/src/edit/boxes.rs`, `crates/tinker-pdf/src/geometry.rs` (new), `crates/tinker-pdf/tests/geometry_edits.rs` (new), `docs/features/editing.md` | `geometry_edits.rs` (new) `boxes_origins_and_inheritance`: each operation's output renders equal to the twin built directly; no page inherits an attribute afterwards. | — |
| ED-09.2 | build | Scale to a paper size, resize and zoom, and add margins or bleed: a matrix before the content and the boxes changed to match. | `crates/tinker-pdf/src/geometry.rs` (new), `crates/tinker-pdf/tests/geometry_edits.rs` (new) | `scale_resize_and_margins`: each operation's output renders equal to the twin built directly at the new size. | ED-09.1 |
| ED-09.3 | build | `/Rotate` flattened into the content: a rotation before the content, the boxes turned, `/Rotate` set to 0. | `crates/tinker-pdf/src/geometry.rs` (new), `crates/tinker-pdf/tests/geometry_edits.rs` (new) | `rotation_flattens_into_content`: pages at 90, 180 and 270 render equal to the original rendered rotated, with `/Rotate` 0. | ED-09.1 |
| ED-09.4 | fix | `/UserUnit` read where the facade measures a page: its size, its render scale and the boxes it reports. | `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/src/render_part.rs`, `crates/tinker-pdf/tests/page_geometry.rs` | `page_geometry.rs` `user_unit_pages_measure_in_their_unit` (beside the `/Rotate` and `/CropBox` tests there): a page with `/UserUnit 2` reports twice the size, renders at twice the pixels at one dpi, and its boxes in its unit; failing first on today's code. | — |
| ED-09.5 | fix | `/UserUnit` read where editing places content: stamps, forms imported from pages and ED-09.2's scaling. | `crates/tinker-pdf-cos/src/edit/stamp.rs`, `crates/tinker-pdf/src/geometry.rs` (new), `crates/tinker-pdf/tests/geometry_edits.rs` (new) | `placements_on_user_unit_pages`: a stamp and an imported page land where the test computes in the target's unit; failing first on today's code. | ED-09.4, ED-09.2 |

**Closes the row:** ED-09.1 to ED-09.3 give every geometry operation a fixture rendering as its twin, and ED-09.4 and ED-09.5 measure a `/UserUnit` page's placements in its unit, which is the row's exit.

## ED-10 — Overlay and underlay with page maps

**Overlay and underlay with page maps**: pages of one document on or under another's, by from/to/repeat maps, as forms, optionally in a layer

**Today.** `import_page_as_form` and `stamp` (`features/editing.md:242-273`), one page at a time

**Exit criterion.** a map applied over generated documents places the expected page on each target, checked by structure and render; `tpdf overlay` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-10.1 | build | Page maps: from, to and repeat maps (every page, odd, even, last, cycling) applying another document's pages as forms over or under a document's pages, underlay drawn before the existing content. | `crates/tinker-pdf/src/overlay.rs` (new), `crates/tinker-pdf-cos/src/edit/stamp.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/page_maps.rs` (new), `docs/features/editing.md` | `page_maps.rs` (new) `maps_place_the_expected_pages`: maps over generated documents put the expected form on each target page, checked by structure and by render. | — |
| ED-10.2 | build | Overlays optionally in a layer: the placed forms wrapped in an optional content group. | `crates/tinker-pdf/src/overlay.rs` (new), `crates/tinker-pdf/tests/page_maps.rs` (new) | `overlays_in_a_layer_toggle`: with the layer off the page renders as before the overlay; on, as with it. | ED-10.1 |
| ED-10.3 | build | `tpdf overlay` with maps and layers, and a fingerprint of one mapped document. | `tools/tpdf/src/writing.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/determinism.rs` | `tpdf overlay` runs over `testdata/` fixtures to the facade's output; the fingerprint holds on every K-07 target. | ED-10.2, K-07 |

**Closes the row:** ED-10.1 and ED-10.2 place the expected page on each target by structure and render, and ED-10.3 runs `tpdf overlay`, which is the row's exit.

## ED-11 — One page-range grammar

**One page-range grammar**: ranges, last page, counting from the end, exclusions, odd and even, collation of interleaved duplex scans

**Today.** `--pages` ranges in the CLI

**Exit criterion.** the grammar's every form held to a table of inputs and page lists

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-11.1 | build | One page-range grammar in the facade: ranges, the last page, counting from the end, exclusions, odd and even, and the collation of interleaved duplex scans, with its syntax in the feature doc. | `crates/tinker-pdf/src/page_ranges.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/page_ranges.rs` (new), `docs/features/editing.md` | `page_ranges.rs` (new) `every_form_of_the_grammar`: a table of inputs and page lists covers every form; malformed inputs are typed errors. | — |
| ED-11.2 | build | The CLI's `--pages` options parsed by the grammar in place of `page_ranges` in `tools/tpdf/src/main.rs`, and a collation command for duplex scans. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | CLI tests run each form of the grammar through `--pages` and the collation command over two-sided generated scans. | ED-11.1 |

**Closes the row:** ED-11.1 holds every form of the grammar to a table of inputs and page lists, which is the row's exit, and ED-11.2 puts it under every `--pages`.

## ED-12 — Split strategies

**Split strategies**: by page count, by top-level outline items (named after titles), by output size, by blank separator pages, by barcode separator pages

**Today.** split by ranges (`tpdf split`)

**Exit criterion.** each strategy over generated documents yields the expected pieces; blank detection uses RC-17

**Doc:** — · **Depends on:** ED-50 (barcodes)

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-12.1 | build | Split by page count, and by top-level outline items with each piece named after its item's title. | `crates/tinker-pdf/src/split.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/split.rs` (new), `docs/features/editing.md` | `split.rs` (new) `count_and_outline_splits`: generated documents yield the expected pieces, pages and names, titles made safe as file names. | — |
| ED-12.2 | build | Split by output size: pieces grown page by page while their written size stays under the limit. | `crates/tinker-pdf/src/split.rs` (new), `crates/tinker-pdf/tests/split.rs` (new) | `size_splits_stay_under_the_limit`: every piece's bytes are under the limit; a single page over it is its own piece with a typed warning. | ED-12.1 |
| ED-12.3 | build | Split by blank separator pages found by RC-17's analysis, the separators dropped. | `crates/tinker-pdf/src/split.rs` (new), `crates/tinker-pdf/tests/split.rs` (new) | `blank_separators_split`: generated documents with blank separators split into the expected pieces. | ED-12.1, RC-17 |
| ED-12.4 | build | Split by barcode separator pages: pages whose decoded barcode (ED-50) matches a value start a piece. | `crates/tinker-pdf/src/split.rs` (new), `crates/tinker-pdf/tests/split.rs` (new) | `barcode_separators_split`: generated documents with barcode separator pages split into the expected pieces. | ED-12.1, ED-50 |
| ED-12.5 | build | `tpdf split --by count\|outline\|size\|blank\|barcode`. | `tools/tpdf/src/writing.rs`, `tools/tpdf/src/main.rs` | CLI tests run each strategy over generated documents to the facade's pieces. | ED-12.2, ED-12.3, ED-12.4 |

**Closes the row:** ED-12.1 to ED-12.4 yield the expected pieces for every strategy, blank detection through RC-17, which is the row's exit.

## ED-13 — Merge fidelity

**Merge fidelity**: AcroForms merged with a renaming policy (with `/DR` and `/CO`), structure trees merged (parent tree renumbered, role maps, class maps, ID trees, namespaces), outlines per file or merged, named destinations renamed on conflict, page labels, optional content, attachments, output intents, JavaScript name trees, viewer preferences, links retargeted

**Today.** `import_page`; `merge` deduplicates

**Exit criterion.** a merged document of two forms fills both; a merged tagged document passes the PDF/UA checks each source passed; links land on the right pages

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-13.1 | design | Create `docs/design/merge.md`: the per-source object map, what each catalog structure becomes when two sources carry it, the renaming policy on conflict, and what is kept or warned when values disagree (output intents, viewer preferences). | `docs/design/merge.md` (new) | The doc exists in the design-doc order, names ED-13.2 to ED-13.8's exit tests, and is reviewed. | — |
| ED-13.2 | build | A facade merge built on `import_page` with a per-source object map, so links and outline destinations are retargeted to the merged pages. | `crates/tinker-pdf/src/merge.rs` (new), `crates/tinker-pdf-cos/src/edit/page_ops.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/merge.rs` (new) | `merge.rs` (new) `links_land_on_the_right_pages`: links and destinations in both sources land on their own pages in the merged document. | ED-13.1 |
| ED-13.3 | build | Name trees merged with renaming on conflict: destinations, embedded files and document JavaScript. | `crates/tinker-pdf/src/merge.rs` (new), `crates/tinker-pdf-cos/src/edit/trees.rs`, `crates/tinker-pdf/tests/merge.rs` (new) | `name_trees_merge_and_rename`: each tree holds both sources' entries, a conflicting name renamed by the policy, and every link to a renamed destination still resolves. | ED-13.2 |
| ED-13.4 | build | AcroForms merged: fields renamed on conflict by the policy, `/DR` resources merged, `/CO` joined. | `crates/tinker-pdf/src/merge.rs` (new), `crates/tinker-pdf-cos/src/edit/forms.rs`, `crates/tinker-pdf/tests/merge.rs` (new) | `two_forms_merge_and_fill`: a merged document of two forms fills both through the form API, calculation order kept, appearances regenerated. | ED-13.2 |
| ED-13.5 | build | Structure trees merged: the roots' children joined, the parent tree renumbered, role maps, class maps, ID trees and namespaces merged with renaming on conflict. | `crates/tinker-pdf/src/merge.rs` (new), `crates/tinker-pdf/src/structure.rs`, `crates/tinker-pdf/tests/merge.rs` (new) | `a_merged_tagged_document_passes_what_each_source_passed`: the merged document passes every PDF/UA check each tagged source passed. | ED-13.2 |
| ED-13.6 | build | Outlines per file (each source under an item named for it) or merged, and page labels joined with their ranges offset. | `crates/tinker-pdf/src/merge.rs` (new), `crates/tinker-pdf-cos/src/edit/docops.rs`, `crates/tinker-pdf/tests/merge.rs` (new) | `outlines_and_labels_merge`: both outline modes and the labels read back as the test computes. | ED-13.2 |
| ED-13.7 | build | Optional content, output intents and viewer preferences: OCGs and configurations joined, an identical output intent kept once and a differing one warned, the first source's viewer preferences kept. | `crates/tinker-pdf/src/merge.rs` (new), `crates/tinker-pdf/tests/merge.rs` (new) | `layers_intents_and_preferences_merge`: each kind reads back as the design states, the warning typed. | ED-13.2 |
| ED-13.8 | build | `tpdf merge` takes the renaming policy and the outline mode, and a fingerprint of one merged document. | `tools/tpdf/src/writing.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/determinism.rs`, `docs/features/editing.md` | CLI tests merge two forms and two tagged documents to the facade's output; the fingerprint holds on every K-07 target. | ED-13.3, ED-13.4, ED-13.5, ED-13.6, ED-13.7, K-07 |

**Closes the row:** ED-13.4 fills both forms, ED-13.5 passes the PDF/UA checks each source passed and ED-13.2 lands links on the right pages, which is the row's exit; ED-13.3, ED-13.6 and ED-13.7 carry the rest of the catalog.

## ED-14 — Bates numbering

**Bates numbering** across an ordered set of files, with prefix, suffix, digits, placement and a range report, removable

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'bates' crates tools` finds nothing

**Exit criterion.** numbers continuous across files; the report matches; removal restores the render; `tpdf bates` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-15

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-14.1 | build | Bates numbering over an ordered set of documents through ED-15's tokens: prefix, suffix, digits and placement, numbers continuous across files, a range report per file, written as removable artifacts. | `crates/tinker-pdf/src/bates.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/bates.rs` (new), `docs/features/editing.md` | `bates.rs` (new) `numbers_run_on_across_files`: numbers are continuous across three generated files and the report matches each file's first and last. | ED-15 |
| ED-14.2 | build | Removal, `tpdf bates` and a fingerprint of one numbered page. | `crates/tinker-pdf/src/bates.rs` (new), `tools/tpdf/src/writing.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/bates.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | `removal_restores_the_render`: removing the numbers renders byte-identically to the original; `tpdf bates` runs over `testdata/` fixtures; the fingerprint holds on every K-07 target. | ED-14.1, K-07 |

**Closes the row:** ED-14.1 numbers continuously with a matching report and ED-14.2 removes the numbers and runs `tpdf bates`, which is the row's exit.

## ED-15 — Headers, footers and page numbers

**Headers, footers and page numbers** from tokens (page n of N, dates, file name, Bates), per range and odd/even, shaped and embedded, written as `/Artifact /Pagination` so PDF/UA stays valid, recorded so they can be updated or removed

**Today.** `stamp` places one form

**Exit criterion.** outputs pass the PDF/UA artifact rules; update and removal restore the original render; `tpdf header-footer` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-45

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-15.1 | build | Tokens and their text: page n of N, dates, file name and a Bates counter, expanded per page and per range with odd and even variants. | `crates/tinker-pdf/src/pagination.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/pagination.rs` (new) | `pagination.rs` (new) `tokens_expand_per_page`: a table of token strings, ranges and pages gives the text the test expects. | — |
| ED-15.2 | build | Headers and footers drawn: the expanded text shaped and embedded through ED-45, placed by position and margin, and written as `/Artifact /Pagination` marked content. | `crates/tinker-pdf/src/pagination.rs` (new), `crates/tinker-pdf/tests/pagination.rs` (new), `docs/features/editing.md` | `headers_and_footers_pass_the_artifact_rules`: outputs on tagged fixtures pass the PDF/UA artifact rules in `crates/tinker-pdf/src/pdfua`, and their text sits where the test computes, in both builds. | ED-15.1, ED-45 |
| ED-15.3 | build | Recorded for update and removal: each header and footer marked so it can be found again, replaced with new tokens, or removed. | `crates/tinker-pdf/src/pagination.rs` (new), `crates/tinker-pdf/tests/pagination.rs` (new) | `update_and_removal_restore_the_render`: updating changes only the header text; removing renders byte-identically to the original. | ED-15.2 |
| ED-15.4 | build | `tpdf header-footer` and a fingerprint of one paginated page. | `tools/tpdf/src/writing.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/determinism.rs` | `tpdf header-footer` runs over `testdata/` fixtures to the facade's output; the fingerprint holds on every K-07 target. | ED-15.3, K-07 |

**Closes the row:** ED-15.2 passes the PDF/UA artifact rules, ED-15.3 restores the render on update and removal and ED-15.4 runs `tpdf header-footer`, which is the row's exit.

## ED-16 — Watermarks and backgrounds that can be updated or removed

**Watermarks and backgrounds that can be updated or removed**: text, image or page sources, rotation, opacity, front or behind, as a `/Watermark` annotation with `/FixedPrint` or as layered content

**Today.** `stamp`

**Exit criterion.** removal restores the original render; print/view usage honoured (RC-13); `tpdf watermark` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-15

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-16.1 | build | Watermarks and backgrounds from text, image or page sources, with rotation, opacity and front or behind, written as a `/Watermark` annotation with `/FixedPrint` or as layered content, recorded through ED-15's marking so they can be updated or removed. | `crates/tinker-pdf/src/watermark.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/watermark.rs` (new), `docs/features/editing.md` | `watermark.rs` (new) `watermarks_place_and_remove`: each source and placement renders as the test composes it; removal renders byte-identically to the original. | ED-15 |
| ED-16.2 | build | Print and view usage honoured: a watermark marked for print only or view only drawn by RC-13's usage, and `tpdf watermark`. | `crates/tinker-pdf/src/watermark.rs` (new), `crates/tinker-pdf/tests/watermark.rs` (new), `tools/tpdf/src/writing.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/determinism.rs` | `usage_is_honoured`: a print-only watermark renders under print usage and not view, and the reverse; `tpdf watermark` runs over `testdata/` fixtures; a fingerprint holds on every K-07 target. | ED-16.1, RC-13, K-07 |

**Closes the row:** ED-16.1 removes to the original render and ED-16.2 honours print and view usage and runs `tpdf watermark`, which is the row's exit.

## ED-17 — Outline styling and document settings

**Outline styling and document settings**: outline item colour and style (`/C`, `/F`) read and written; outline import and export as JSON and text; outlines generated from structure headings or font-size heuristics; `/Lang`, `/PageMode`, `/PageLayout` and `/MarkInfo` set on an existing document

**Today.** outline read and written without `/C` or `/F` (`crates/tinker-pdf-cos/src/outline.rs` reads neither); `/Lang` set by the builder only

**Exit criterion.** fixtures for each key read and written; a generated outline matches the tagged headings of the committed books; `tpdf bookmarks`, `tpdf labels` and `tpdf metadata`, get and set runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-17.1 | build | Outline item colour and style (`/C`, `/F`) read by `outline.rs` and written by `set_outline`. | `crates/tinker-pdf-cos/src/outline.rs`, `crates/tinker-pdf-cos/src/edit/docops.rs`, `crates/tinker-pdf/tests/outline_settings.rs` (new) | `outline_settings.rs` (new) `colour_and_style_round_trip`: items written with each `/C` and every `/F` bit read back as written; failing first on today's reader. | — |
| ED-17.2 | build | Outline import and export as JSON and as indented text. | `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/outline_settings.rs` (new) | `outlines_export_and_import`: an outline exported in each form and imported again is the same outline, titles, destinations, colours and styles. | ED-17.1 |
| ED-17.3 | build | Outlines generated from structure headings (`H`, `H1`…`H6`) or, on untagged pages, from a font-size heuristic, labelled as generated. | `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/src/structure.rs`, `crates/tinker-pdf/tests/outline_settings.rs` (new) | `a_generated_outline_matches_the_tagged_headings`: on the committed books the generated outline equals their tagged headings. | ED-17.2 |
| ED-17.4 | build | Document settings on an existing document: `/Lang`, `/PageMode`, `/PageLayout` and `/MarkInfo` set through `DocumentEditor`. | `crates/tinker-pdf-cos/src/edit/docops.rs`, `crates/tinker-pdf-cos/src/edit/view.rs`, `crates/tinker-pdf/tests/outline_settings.rs` (new) | `settings_round_trip`: each key set on a fixture reads back as written and validates strictly. | — |
| ED-17.5 | build | `tpdf bookmarks`, `tpdf labels` and `tpdf metadata`, each with get and set. | `tools/tpdf/src/writing.rs`, `tools/tpdf/src/main.rs`, `docs/features/editing.md` | CLI tests get and set each over `testdata/` fixtures and read the result back through the facade. | ED-17.2, ED-17.4 |

**Closes the row:** ED-17.1 and ED-17.4 read and write every key, ED-17.3 matches the committed books' tagged headings, and ED-17.5 runs the three CLI commands, which is the row's exit.

## ED-18 — A table-of-contents page

**A table-of-contents page** with leaders, page numbers and links generated from the outline

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'table_of_contents|toc_page' crates tools` finds nothing

**Exit criterion.** the generated page's links land on their targets; text extracts as the outline

**Doc:** — · **Depends on:** ED-45

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-18.1 | build | A table-of-contents page generated from the outline: titles shaped through ED-45, indented by level, dot leaders and page numbers, inserted at a given index. | `crates/tinker-pdf/src/toc.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/toc.rs` (new), `docs/features/editing.md` | `toc.rs` (new) `the_contents_page_reads_as_the_outline`: the page's text extracts as the outline's titles and page numbers, in both builds. | ED-45 |
| ED-18.2 | build | Links on each entry to its target, and the contents spilling onto further pages when long. | `crates/tinker-pdf/src/toc.rs` (new), `crates/tinker-pdf/tests/toc.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | `entries_link_to_their_targets`: every entry's link lands on its outline target, page numbers shifted for the inserted pages; a fingerprint holds on every K-07 target. | ED-18.1, K-07 |

**Closes the row:** ED-18.1 extracts as the outline and ED-18.2 links land on their targets, which is the row's exit.

## ED-19 — Article threads and page transitions

**Article threads and page transitions**: `/Threads` and beads read, followed and written; `/Trans` and `/Dur` read and written, and transition frames generated for a presenting host

**Today.** absent — the owner's parity decision of 9 October 2026; `/Threads` is read nowhere, and `/Trans` is named only in `sanitise`'s list of the actions that stay (`crates/tinker-pdf-cos/src/edit/sanitise.rs:86, 210`)

**Exit criterion.** fixtures read back to what was written; transition frames equal the 12.4.4 effect computed in the test

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-19.1 | build | Article threads: `/Threads` and their beads read and followed in order, and written. | `crates/tinker-pdf-cos/src/threads.rs` (new), `crates/tinker-pdf-cos/src/lib.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/threads.rs` (new) | `threads.rs` (new) `threads_read_back_as_written`: fixtures with threads across pages read back to what was written, beads in order, and validate strictly. | — |
| ED-19.2 | build | Page transitions: `/Trans` and `/Dur` read and written. | `crates/tinker-pdf-cos/src/edit/page_ops.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/threads.rs` (new) | `transitions_read_back_as_written`: every 12.4.4 style with its parameters reads back as written. | — |
| ED-19.3 | build | Transition frames for a presenting host: frame n of a transition between two rendered pages, computed as 12.4.4 describes each effect. | `crates/tinker-pdf/src/transitions.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/threads.rs` (new), `docs/features/rendering.md` | `frames_equal_the_effect`: each style's frames equal the effect the test computes from the two renders. | ED-19.2 |

**Closes the row:** ED-19.1 and ED-19.2 read back what they wrote and ED-19.3's frames equal 12.4.4's effect, which is the row's exit.

## ED-20 — Embedded page thumbnails

**Embedded page thumbnails**: read `/Thumb`, generate, embed, remove

**Today.** `/Thumb` appears only in the linearization rules

**Exit criterion.** a generated thumbnail equals the page rendered at the thumbnail scale

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-20.1 | build | Thumbnails: `/Thumb` read as an image, generated by rendering the page at the thumbnail scale, embedded, and removed. | `crates/tinker-pdf/src/thumbnails.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf-cos/src/edit/page_ops.rs`, `crates/tinker-pdf/tests/thumbnails.rs` (new), `docs/features/editing.md` | `thumbnails.rs` (new) `a_generated_thumbnail_is_the_page_at_its_scale`: the embedded thumbnail's samples equal the page rendered at the thumbnail scale; removal leaves no `/Thumb`. | — |

**Closes the row:** ED-20.1 reads, generates, embeds and removes thumbnails, a generated one equal to the page at its scale, which is the row's exit.

## ED-21 — Portfolios

**Portfolios**: the `/Collection` schema, item metadata, folders, sort, initial document and view mode read, created, edited and extracted

**Today.** attachments listed and written; `/Collection` not read

**Exit criterion.** a portfolio built and re-read yields its schema, folders and files byte for byte

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-21.1 | build | Portfolios read: `/Collection`'s schema, item metadata, folders, sort order, initial document and view mode. | `crates/tinker-pdf-cos/src/collection.rs` (new), `crates/tinker-pdf-cos/src/lib.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/portfolio.rs` (new) | `portfolio.rs` (new) `a_portfolio_reads_its_schema_and_folders`: a builder portfolio's schema, folders, sort and view read as the builder wrote them. | — |
| ED-21.2 | build | Portfolios created and edited: schema fields, item metadata, folders and files added, moved and removed. | `crates/tinker-pdf-cos/src/collection.rs` (new), `crates/tinker-pdf-cos/src/edit/docops.rs`, `crates/tinker-pdf/tests/portfolio.rs` (new) | `a_built_portfolio_re_reads_byte_for_byte`: a portfolio built, saved and re-read yields its schema, folders and files byte for byte, and validates strictly. | ED-21.1 |
| ED-21.3 | build | Extraction of a portfolio's files with their folder paths, from the facade and the CLI. | `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/portfolio.rs` (new), `docs/features/editing.md` | `files_extract_under_their_folders`: every file extracts byte for byte under its folder path. | ED-21.2 |

**Closes the row:** ED-21.2 builds a portfolio that re-reads byte for byte, read by ED-21.1 and extracted by ED-21.3, which is the row's exit.

## ED-22 — Document comparison, text

**Document comparison, text**: a word-level diff across documents or revisions with insert, delete, replace, format and move classes, as a report and as annotations

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\btext_diff\b|compare_documents|\bdocument_diff\b' crates tools` finds nothing

**Exit criterion.** generated document pairs with known edits yield exactly those edits; `tpdf compare --text` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-22.1 | build | A word-level diff of two documents' extracted text, in reading order, with insert, delete and replace classes and each change's quads on both sides. | `crates/tinker-pdf/src/compare.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/compare_text.rs` (new) | `compare_text.rs` (new) `known_edits_are_found`: generated pairs with known inserts, deletes and replaces yield exactly those edits. | — |
| ED-22.2 | build | Move and format classes: a block found deleted in one place and inserted in another reported as a move; the same words in another font, size or colour reported as a format change. | `crates/tinker-pdf/src/compare.rs` (new), `crates/tinker-pdf/tests/compare_text.rs` (new) | `moves_and_format_changes_are_classed`: generated pairs with known moves and format changes yield exactly those. | ED-22.1 |
| ED-22.3 | build | Revisions as inputs (ED-31's revision N against the current), the report as JSON and text, and the changes written as annotations. | `crates/tinker-pdf/src/compare.rs` (new), `crates/tinker-pdf/tests/compare_text.rs` (new) | `reports_and_annotations`: the report re-reads to the edits; the annotated output marks each change's quads and validates strictly. | ED-22.2, ED-31 |
| ED-22.4 | build | `tpdf compare --text` and a fingerprint of one report. | `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/determinism.rs`, `docs/features/content-and-text.md` | `tpdf compare --text` runs over `testdata/` fixture pairs to the facade's report; the fingerprint holds on every K-07 target. | ED-22.3, K-07 |

**Closes the row:** ED-22.1 and ED-22.2 find exactly the known edits in every class and ED-22.4 runs `tpdf compare --text`, which is the row's exit.

## ED-23 — Document comparison, visual

**Document comparison, visual**: pixel and vector overlay diff with colour coding, thresholds, shifted-page alignment and ignored regions

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'visual_diff|pixel_diff|compare_renders' crates tools` finds nothing

**Exit criterion.** generated pairs with known differences report exactly those regions; identical renders report none; `tpdf compare --visual` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-23.1 | build | A pixel diff of two pages rendered at one resolution: differences above a threshold grouped into connected regions, reported as rectangles. | `crates/tinker-pdf/src/compare.rs` (new), `crates/tinker-pdf/tests/compare_visual.rs` (new) | `compare_visual.rs` (new) `known_differences_are_the_regions_reported`: generated pairs with known changed areas report exactly those regions; identical renders report none. | ED-22 |
| ED-23.2 | build | Ignored regions and the colour-coded overlay: rectangles left out of the comparison, and an overlay image marking what only one side draws in its own colour. | `crates/tinker-pdf/src/compare.rs` (new), `crates/tinker-pdf/tests/compare_visual.rs` (new) | `ignored_regions_and_the_overlay`: a change inside an ignored region is never reported; the overlay's pixels equal those the test computes from the two renders. | ED-23.1 |
| ED-23.3 | build | Shifted-page alignment: a translation between the two pages estimated from their renders and applied before the diff. | `crates/tinker-pdf/src/compare.rs` (new), `crates/tinker-pdf/tests/compare_visual.rs` (new) | `shifted_pages_align`: a page shifted by known offsets reports no difference; a shifted page with one real change reports only that change. | ED-23.1 |
| ED-23.4 | build | A vector overlay diff: the two pages' objects (ED-01) matched by kind and geometry, and added, removed and moved objects reported as regions. | `crates/tinker-pdf/src/compare.rs` (new), `crates/tinker-pdf/tests/compare_visual.rs` (new) | `vector_changes_are_found`: generated pairs with known added, removed and moved paths, images and text runs report exactly those objects' regions. | ED-23.1, ED-01 |
| ED-23.5 | build | `tpdf compare --visual` and a fingerprint of one overlay. | `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/determinism.rs`, `docs/features/rendering.md` | `tpdf compare --visual` runs over `testdata/` fixture pairs to the facade's report; the fingerprint holds on every K-07 target. | ED-23.2, ED-23.3, ED-23.4, K-07 |

**Closes the row:** ED-23.1 to ED-23.4 report exactly the known regions and nothing for identical renders, and ED-23.5 runs `tpdf compare --visual`, which is the row's exit.

## ED-24 — Optimisation presets and font unification

**Optimisation presets and font unification**: screen, ebook, print and prepress presets; identical-but-differently-subset fonts across merged documents unified; embedded standard-14 programs dropped on request; content minified (number precision, operator coalescing)

**Today.** subsetting on rewrite by default, stream deduplication, image recoding (`SaveOptions::images`)

**Exit criterion.** each preset's output validates and renders within its stated tolerance; unification leaves one program per face; savings recorded on the committed fixtures; `tpdf optimise` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** FT-11

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-24.1 | design | Create `docs/design/optimisation.md`: what each preset (screen, ebook, print, prepress) sets, the render tolerance each states, how two subsets of one face are recognised and unified, when a standard-14 program may be dropped, and what content minification may change. | `docs/design/optimisation.md` (new) | The doc exists in the design-doc order, states each preset's settings and tolerance, names ED-24.2 to ED-24.6's exit tests, and is reviewed. | — |
| ED-24.2 | build | The four presets as bundles of save options: image recoding and resolution, deduplication, subsetting and object streams. | `crates/tinker-pdf/src/write.rs`, `crates/tinker-pdf/tests/optimise.rs` (new), `docs/features/writing.md` | `optimise.rs` (new) `each_preset_validates_and_renders_within_its_tolerance`: every committed fixture under each preset validates strictly and renders within the preset's stated tolerance. | ED-24.1 |
| ED-24.3 | build | Font unification: two subsets of one face across merged documents replaced by one program holding the union of their glyphs, through FT-11's subsetting. | `crates/tinker-pdf/src/subset.rs`, `crates/tinker-pdf/tests/optimise.rs` (new) | `unification_leaves_one_program_per_face`: a merge of documents subsetting one face differently keeps one program per face, and every page renders byte-identically. | ED-24.1, FT-11 |
| ED-24.4 | build | Embedded standard-14 programs dropped on request, the font left a standard-14 reference when its name and metrics match. | `crates/tinker-pdf/src/write.rs`, `crates/tinker-pdf/tests/optimise.rs` (new) | `standard_14_programs_drop`: a fixture embedding a standard-14 face loses the program and renders within the design's tolerance; a face whose metrics differ keeps its program. | ED-24.1 |
| ED-24.5 | build | Content minified through ED-04's pipeline: numbers at a stated precision and adjacent operators coalesced. | `crates/tinker-pdf-content/src/filter.rs` (new), `crates/tinker-pdf/tests/optimise.rs` (new) | `minified_content_renders_within_tolerance`: minified fixtures render within the design's tolerance and are smaller by the bytes the test counts. | ED-24.1, ED-04 |
| ED-24.6 | measure | `tpdf optimise --preset`, the savings on the committed fixtures recorded in the feature doc, and a fingerprint of one preset's output. | `tools/tpdf/src/writing.rs`, `tools/tpdf/src/main.rs`, `docs/features/writing.md`, `crates/tinker-pdf/tests/determinism.rs` | `tpdf optimise` runs every preset over `testdata/` fixtures; the feature doc quotes each preset's measured savings; the fingerprint holds on every K-07 target. | ED-24.2, ED-24.3, ED-24.4, ED-24.5, K-07 |

**Closes the row:** ED-24.2 validates and renders each preset within tolerance, ED-24.3 leaves one program per face, and ED-24.6 records the savings and runs `tpdf optimise`, which is the row's exit.

## ED-25 — Image recompression remainder

**Image recompression remainder**: (1) images stored through an image codec recoded, once the read side hands their samples in their own colour space; (2) images drawn only by a pattern cell, a Type 3 glyph or a soft mask resampled; average and bicubic resampling and a threshold resolution beside the box filter

**Today.** **Landed for images stored through the general filters** — `tinker_pdf::write::SaveOptions::images`, off by default, recodes per kind and box-filters to a caller's `max_ppi`, held by `crates/tinker-pdf/tests/image_recode.rs` and surfaced by `tpdf`'s writers as `--images`, `--bilevel` and `--max-ppi` ([features/writing.md](../features/writing.md)). **What is left, by name.** (1) An image stored as `DCTDecode`, `JPXDecode`, `CCITTFaxDecode` or `JBIG2Decode` is `UntouchedImageReason::Filter` (`features/writing.md:353`) — its samples *in their own colour space* are the read side's type, and recoding them waits on that rather than on a second decoder here; a photograph is commonly stored as DCT, so this is the part a downsampling caller is likeliest to meet. (2) An image only a tiling pattern's cell, a Type 3 glyph or a soft mask's group draws is recoded but never resampled (`Unplaced`): the walk interprets pages, forms and appearances only. (Clause (3), `/UserUnit`, is ED-09's.) No corpus measurement was taken: the fetched corpora were not in the container where the half landed

**Exit criterion.** as the landed half: lossless codings return the samples written, the page draws the same pixels, JPEG within the bound computed from the caller's table; resampling equals the filter computed in the test; the corpus measurement taken where the corpus is

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-25.1 | build | The read side hands the writer an image's samples in its own colour space for `DCTDecode`, `JPXDecode`, `CCITTFaxDecode` and `JBIG2Decode`, with its precision and any refusal typed. | `crates/tinker-pdf/src/images.rs`, `crates/tinker-pdf/src/recode.rs`, `crates/tinker-pdf/tests/image_recode.rs` | `image_recode.rs` `codec_images_read_in_their_own_space`: each codec's fixture returns the samples and colour space the filters crate decodes, before any conversion for display. | — |
| ED-25.2 | build | Images stored through an image codec recoded and resampled, retiring `UntouchedImageReason::Filter` for them. | `crates/tinker-pdf/src/recode.rs`, `crates/tinker-pdf/tests/image_recode.rs`, `docs/features/writing.md` | `codec_images_recode`: lossless codings return the samples written and the page draws the same pixels; JPEG output is within the bound computed from the caller's table; failing first on today's `Filter` refusal. | ED-25.1 |
| ED-25.3 | build | Images drawn only by a tiling pattern's cell, a Type 3 glyph or a soft mask's group placed by the walk, so they are resampled rather than left `Unplaced`. | `crates/tinker-pdf/src/recode.rs`, `crates/tinker-pdf/tests/image_recode.rs` | `images_in_patterns_glyphs_and_masks_resample`: each case is resampled at its drawn resolution and equals the box filter computed in the test; failing first on today's `Unplaced`. | — |
| ED-25.4 | build | Average and bicubic resampling and a threshold resolution beside the box filter. | `crates/tinker-pdf/src/recode.rs`, `crates/tinker-pdf/src/write.rs`, `crates/tinker-pdf/tests/image_recode.rs`, `docs/features/writing.md` | `every_filter_equals_its_computation`: each filter's output equals the filter computed in the test; an image under the threshold is left alone. | ED-25.3 |
| ED-25.5 | measure | The corpus measurement: a census in `corpus.yml` recoding every image of the corpora and recording the bytes saved and each refusal by reason. | `crates/tinker-pdf/tests/image_recode_census.rs` (new), `.github/workflows/corpus.yml`, `docs/features/writing.md` | The census prints `RAN` and its figures on a dispatched run on `develop`, and the feature doc quotes them with the run. | ED-25.2, ED-25.4, MS-11, H-03 |

**Closes the row:** ED-25.2 recodes codec-stored images and ED-25.3 resamples pattern, glyph and mask images, each held as the landed half is, ED-25.4 adds the filters, and ED-25.5 takes the corpus measurement, which is the row's exit.

## ED-26 — A JPEG 2000 encoder

**A JPEG 2000 encoder** (reversible and irreversible, tiles, layers, progression orders)

**Today.** absent — the owner's parity decision of 9 October 2026; `tinker-pdf-filters` writes PNG, CCITT G4, JBIG2 generic regions and baseline JPEG and no JPEG 2000 (`rg -i 'jpx_encode|encode_jpx' crates tools` finds nothing)

**Exit criterion.** T.800 Annex J.10's published image encodes to J.10's published codestream with J.10's parameters; every other configuration decodes through the in-tree decoder within the bound computed in the test (lossless exactly)

**Doc:** `design/jpx-encoder.md` (owed) · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-26.1 | design | Write `docs/design/jpx-encoder.md` (scope, non-goals, design, milestones with exit tests, risks): the `jpx_encode` surface (a borrowed raster with a stride, plain numbers, a refusal enum; ruling 8), J.10's encoder choices read off its published bytes, the 5/3 and fixed-point 9/7 analysis, RCT and ICT, the scalar quantiser with caller-named steps and no quality number, tier-1 and T.800's termination, the packet writer, tiles, precincts, layers, the five progression orders, the JP2 boxes, and the irreversible bound's derivation. *Note:* Non-goals, each with its reason: what the Item does not list (ROI and POC writing, SOP/EPH, PPM/PPT, Table A.19's styles beyond the default, tile-part division), Part 2 (FI-13), and any caller outside the tests (a writer choosing JPX is H-20's policy). Integer arithmetic only (ruling 4); `tinker-pdf-filters` is not on `cargo xtask libm`'s list (`xtask/src/main.rs:840-846`), so ED-26.15's fingerprint is the cross-target check. Cite committed reference decodes by file, never by the program that made them. | `docs/design/jpx-encoder.md` (new) | The doc exists in that order and settles, before ED-26.2 starts: the marker order, `TNsot`, pass counts, MQ termination and length signalling J.10 uses, each checked against J.10.3 and J.10.4's published bytes; the 9/7 analysis's fixed-point format and overflow argument; the per-sample bound the irreversible tests compute, derived from the written step sizes, the synthesis filters and every rounding; the layer allocation rule in integers; the precisions and colours `JpxSource` takes; ED-26.2 to ED-26.15's exit tests; and it is reviewed. | — |
| ED-26.2 | build | Ship tier-1's encoder: `tier1::encoder::encode_code_block` leaves `cfg(test)` and codes a code-block's every pass over the decoder's own context formation (D.2's three passes, Table D.7's initial states), and `MqEncoder` gains T.800's termination as ED-26.1 reads it off J.10 beside T.88's FLUSH, which keeps its `FF AC`. *Note:* Context formation stays shared with the decoder, so J.10's bytes and the transcribed Annex D tables are the evidence, not the round trip (gap 17's limit, `jpx/tier1.rs:983-996`). P-10.5 packs the decoder's per-coefficient `Block` and keeps the transcribed one for this encoder; whichever lands second keeps the encoder on its own state. `cargo test -p tinker-pdf-filters`. Inject a wrong Table D.7 state and a dropped trailing byte; count what fires. | `crates/tinker-pdf-filters/src/jpx/tier1.rs`, `crates/tinker-pdf-filters/src/mq.rs`, `crates/tinker-pdf-filters/src/jpx/tests/mod.rs`, `crates/tinker-pdf-filters/src/jpx/tests/encode_tier1.rs` (new) | `jpx/tests/encode_tier1.rs` (new) `j10_4s_coefficients_encode_to_j10s_code_block_bytes`: J.10.4's low-pass coefficients -26, -22, -30, -32, -19 and high-pass 1, 5, 1, 0, at the bit-planes J.10's QCD and packet headers give, encode in sixteen and seven passes to J.10's published `01 8F 0D C8 75 5D` and `0F B1 76`, failing first; `mq.rs`'s Annex H.2 tests and the existing tier-1 round trips pass unchanged. | ED-26.1 |
| ED-26.3 | build | The packet writer (B.9, B.10): a packet header's zero-length bit (B.10.3), inclusion coded against the layer's threshold on a tag tree that keeps its state across layers (B.10.4), the zero-bit-plane tree (B.10.5), the pass-count codes (B.10.6), the least `Lblock` increment that fits and the lengths (B.10.7.1), with B.10.1's stuffing, then the body; `encode_tag_tree_for_test` and its `BitWriter` (`jpx/tier2.rs:397-507`) give way to it. *Note:* The test-only tag-tree encoder codes each leaf to completion, which a multi-layer header must not do. `tests/tier2.rs`'s three tag-tree tests move onto the shipped encoder and the test-only one is deleted with its `BitWriter` (CI rejects dead code). Inject an `Lblock` increment one short and the stuffing's seven-bit byte read as eight; count what fires. | `crates/tinker-pdf-filters/src/jpx/tier2.rs`, `crates/tinker-pdf-filters/src/jpx/tests/mod.rs`, `crates/tinker-pdf-filters/src/jpx/tests/encode_tier2.rs` (new), `crates/tinker-pdf-filters/src/jpx/tests/tier2.rs` | `jpx/tests/encode_tier2.rs` (new): `j10s_packet_headers_are_tables_j20_and_j21s_bytes` writes `C7 D4 0C` and `C0 7C 21 80` from J.10's two code-block contributions; `b10_7_1s_four_layers_are_the_printed_bits` writes NOTE 1's four sequences for 6, 31, 44 and 134 bytes over 1, 9, 2 and 5 passes; `b10_2s_worked_tag_tree_is_the_printed_bit_string`; each fails first, and every packet written reads back through `tier2`'s own reader. | ED-26.2 |
| ED-26.4 | build | The forward wavelet: one analysis ladder over the two arithmetics the decoder's synthesis already separates, the 5/3 integer lifting (exact) and the 9/7 lifting in ED-26.1's fixed-point format on Table F.4's Q24 constants, over every tile-component shape and level count, with Annex G's DC level shift in front. *Note:* The 9/7 analysis's overflow argument goes into `wavelet.rs`'s header beside the synthesis one. P-10.7 rewrites the synthesis lifting in the roadmap; the analysis is new code beside it and shares only the constants. Inject the 5/3 update step's rounding offset and the 9/7's two scalings swapped; count what fires. | `crates/tinker-pdf-filters/src/jpx/wavelet.rs`, `crates/tinker-pdf-filters/src/jpx/tests/mod.rs`, `crates/tinker-pdf-filters/src/jpx/tests/analysis.rs` (new) | `jpx/tests/analysis.rs` (new): `j10_5s_samples_analyse_to_j10_4s_coefficients` (the nine published samples give -26, -22, -30, -32, -19 and 1, 5, 1, 0); `the_5_3_analysis_is_inverted_exactly_by_the_ladder` over planes of every parity of origin and size at 0 to 5 levels, one-row and one-column planes included; `the_fixed_point_9_7_analysis_tracks_an_f64_reference` within ED-26.1's derived residual; all failing first. | ED-26.1 |
| ED-26.5 | build | `jpx_encode`: a `JpxSource` raster (width, height, components, precision, signedness, stride, bytes) and `JpxOptions` in, bytes or a `JpxEncodeError` out; the main header (SIZ, QCD, COD in J.10's order), one tile-part per tile (SOT with its `Psot`, SOD), EOC; reversible 5/3 with no quantisation, one tile, one layer, one precinct per resolution, LRCP, chaining ED-26.2 to ED-26.4. *Note:* No facade entry (ruling 11), as for the other four encoders. The headers that say nothing ships JPEG 2000 (`jpx/tests/mod.rs:9`, `jpx/tier1.rs:985`) are corrected; the permissive `jpx/tests/writer.rs` stays for the refusal tests. `docs/features/filters.md`'s encoder paragraphs and API list (`:739-747`) gain `jpx_encode`. | `crates/tinker-pdf-filters/src/jpx/encode.rs` (new), `crates/tinker-pdf-filters/src/jpx/mod.rs`, `crates/tinker-pdf-filters/src/lib.rs`, `crates/tinker-pdf-filters/src/jpx/tests/mod.rs`, `crates/tinker-pdf-filters/src/jpx/tier1.rs`, `crates/tinker-pdf-filters/tests/jpx_annex_j.rs`, `crates/tinker-pdf-filters/tests/jpx_encode.rs` (new), `docs/features/filters.md` | `jpx_annex_j.rs` `j10_5s_samples_encode_to_j10s_published_codestream` (new): the nine samples under J.10's parameters encode to `ANNEX_J10`'s hundred bytes, the row's first half; `jpx_encode.rs` (new) `reversible_encodes_decode_exactly` over generated images of one to four components at every precision ED-26.1 admits, signed and unsigned, at odd sizes, compared at `JpxImage`'s output precision; `every_refusal_fires_by_its_own_name`; all failing first. | ED-26.2, ED-26.3, ED-26.4 |
| ED-26.6 | build | Irreversible coding: Annex E's scalar quantiser between the 9/7 analysis and tier-1, with caller-named steps written as QCD's scalar-derived style (one exponent and mantissa) or scalar-expounded style (one per subband) (A.6.4), the guard bits and the `Mb` each band then has; no quality number, for `jpeg_encode`'s reason (`jpeg/encode.rs:79-102`). *Note:* The bound is computed in the test, never chosen, and no second decoder is asked (ruling 13). The quantiser works in integers on the Q12 plane. Inject a mantissa off by one and the deadzone's rounding toward the wrong side; count what fires. | `crates/tinker-pdf-filters/src/jpx/encode.rs` (new), `crates/tinker-pdf-filters/src/jpx/wavelet.rs`, `crates/tinker-pdf-filters/tests/jpx_encode.rs` (new), `docs/features/filters.md` | `jpx_encode.rs` `irreversible_encodes_decode_within_the_bound_the_test_computes` (new): generated grey and multi-component images at every level count, both QCD styles and several steps, each decoded sample within the bound the test computes from the codestream's own QCD, the synthesis filters and the fixed-point rounding as ED-26.1 derives it; `the_qcd_carries_the_steps_asked_for`; failing first. | ED-26.5 |
| ED-26.7 | build | Annex G's forward component transforms behind COD's multiple-component-transform flag: the RCT with the 5/3 and the ICT with the 9/7 in ED-26.1's fixed-point format, over the first three components, the inverses `colour.rs` already runs (`inverse_rct`, `inverse_ict`). *Note:* The test's forward RCT (`jpx/tests/colour.rs:140-142`) stays the independent transcription the shipped one is held to. Inject the RCT's blue and red differences swapped; count what fires. | `crates/tinker-pdf-filters/src/jpx/colour.rs`, `crates/tinker-pdf-filters/src/jpx/encode.rs` (new), `crates/tinker-pdf-filters/src/jpx/tests/colour.rs`, `crates/tinker-pdf-filters/tests/jpx_encode.rs` (new) | `jpx/tests/colour.rs` `the_shipped_rct_is_the_forward_equation_the_test_writes` (new) holds it to the forward transform already written from G.2.1 in `the_inverse_rct_is_exact_over_the_forward_one`, over the 8-bit cube; `jpx_encode.rs` `rgb_through_the_rct_decodes_exactly` and `rgb_through_the_ict_decodes_within_the_bound` (new), the bound gaining ED-26.1's ICT term; failing first. | ED-26.6 |
| ED-26.8 | build | Tiles: the tile grid of SIZ's `XTsiz`, `YTsiz`, `XTOsiz` and `YTOsiz` with the image offsets, each tile-component's extent on the reference grid under every component's `XRsiz` and `YRsiz`, and each tile analysed, coded and written as its own tile-part in index order. *Note:* Subsampling factors are the caller's, named and never inferred, as `jpeg_encode` treats 4:2:0. A tile written out of order would trip the decoder's tile-part order refusal (`Warning::JpxStructureInvalid`). | `crates/tinker-pdf-filters/src/jpx/encode.rs` (new), `crates/tinker-pdf-filters/tests/jpx_encode.rs` (new), `docs/features/filters.md` | `jpx_encode.rs` `tiled_encodes_decode_exactly_and_within_the_bound` (new): tile sizes that divide the image and that do not, tile and image offsets, one-sample edge tiles and subsampled components, reversible exactly and irreversible within the computed bound; `a_grid_past_a_5_1s_65535_tiles_is_refused_by_name`; failing first. | ED-26.7 |
| ED-26.9 | build | Precincts: COD's `Scod` bit 0 with a `PPx`/`PPy` byte per resolution and one packet per precinct per layer, an empty precinct coded as the zero-length packet, built on the decoder's own B.5 to B.7 geometry (`jpx/tier2.rs:590` onward) rather than a second copy of it. *Note:* The committed reference decodes hold that geometry only at the default precinct (`tests/jpx_reference.rs`'s header lists the settings), so the hand-worked partitions are what pin a non-default one; without them a shared defect would cancel in the round trip. Inject a precinct anchored at the tile rather than the origin; count what fires. | `crates/tinker-pdf-filters/src/jpx/encode.rs` (new), `crates/tinker-pdf-filters/src/jpx/tier2.rs`, `crates/tinker-pdf-filters/src/jpx/tests/encode_tier2.rs` (new), `crates/tinker-pdf-filters/tests/jpx_encode.rs` (new) | `jpx/tests/encode_tier2.rs` `the_written_precincts_are_b_6_and_b_7s_equations` (new): for three tile geometries, odd origins included, the precincts and code-blocks the writer codes equal those the test works out by hand from the equations; `jpx_encode.rs` `precinct_partitions_decode_exactly` (new), from the smallest precinct the code-block size admits to the default, partly empty precincts and subsampled components included; failing first. | ED-26.8 |
| ED-26.10 | build | Quality layers by ED-26.1's allocation rule: each code-block's terminated length after every pass and its distortion reduction counted from its coefficients, the rate-distortion hull of those points, and the caller's per-layer byte budgets met by one integer slope per layer; the packet writer codes each layer's increments. *Note:* Integer slopes and lengths, so the allocation cannot differ between targets (ruling 4). `jpx/tests/code_block_styles.rs`'s `a_contribution_split_across_two_layers_decodes_as_one` already holds the decoder's multi-layer merge. Inject a slope compared the wrong way and a budget overshot by one pass; count what fires. | `crates/tinker-pdf-filters/src/jpx/encode.rs` (new), `crates/tinker-pdf-filters/src/jpx/tier1.rs`, `crates/tinker-pdf-filters/src/jpx/tests/encode_tier1.rs` (new), `crates/tinker-pdf-filters/tests/jpx_encode.rs` (new), `docs/features/filters.md` | `jpx_encode.rs` `layered_encodes_meet_their_budgets_and_decode_alike` (new): one to eight layers, each layer's cumulative bytes within its budget and the whole codestream decoding exactly (reversible) or within the bound; `jpx/tests/encode_tier1.rs` `the_chosen_truncation_points_are_on_the_hull` recomputes each code-block's hull in the test; failing first. | ED-26.9 |
| ED-26.11 | build | The five progression orders of B.12.1 (LRCP, RLCP, RPCL, PCRL, CPRL) sequencing each tile's packets, the two position-driven orders walking precincts on the reference grid, the loops written by the encoder rather than borrowed from the decoder's `packet_order`. *Note:* Writing through `packet_order` (`jpx/tier2.rs:1059`) would let a shared defect cancel, so the two are compared rather than shared; `jpx_poc.rs`'s swap of J.10's two published packets stays the published adjudicator of packet order. Inject PCRL's component and position loops swapped; count what fires. | `crates/tinker-pdf-filters/src/jpx/encode.rs` (new), `crates/tinker-pdf-filters/src/jpx/tier2.rs`, `crates/tinker-pdf-filters/src/jpx/tests/encode_tier2.rs` (new), `crates/tinker-pdf-filters/tests/jpx_encode.rs` (new), `docs/features/filters.md` | `jpx/tests/encode_tier2.rs` `the_written_packet_sequence_is_the_one_packet_order_reads` (new) for every order over multi-component, multi-resolution, multi-precinct, multi-layer tiles, failing first for every order but LRCP; `jpx_encode.rs` `every_progression_order_decodes_alike` (new): one image in each order decodes to the same samples, exactly when reversible. | ED-26.10 |
| ED-26.12 | build | The JP2 file of Annex I, chosen by `JpxOptions` beside the bare codestream: the signature box, `ftyp` with brand `jp2 `, `jp2h` holding `ihdr` and an enumerated `colr` (sRGB or greyscale, as the source declares), and the codestream in `jp2c`. *Note:* `jpx/tests/writer.rs`'s `jp2` is the permissive test writer and stays; this one is strict and refuses a colour it cannot name. Inject `ihdr`'s height and width swapped; count what fires. | `crates/tinker-pdf-filters/src/jpx/encode.rs` (new), `crates/tinker-pdf-filters/tests/jpx_encode.rs` (new), `docs/features/filters.md` | `jpx_encode.rs` `a_jp2_file_carries_its_boxes_where_annex_i_puts_them` (new): each box's length and type at its offset, `ihdr`'s fields equal to SIZ's; `jpx_header` and `jpx_decode` report `JpxColour::Srgb` or `JpxColour::Greyscale` and the bare codestream's samples; failing first. | ED-26.11 |
| ED-26.13 | test | The configuration sweep the row's second half names: a harness over the cross product ED-26.1 lists (transform, components and precision, tiles, precincts, code-block size, levels, layers, progression order, bare or JP2), each encoded, decoded through `jpx_decode`, and compared exactly or against the bound the test computes from the codestream's own QCD. *Note:* Sized to finish in the suite on every leg; the generator is deterministic so a failing configuration is named and reproducible. | `crates/tinker-pdf-filters/tests/jpx_encode.rs` (new) | `jpx_encode.rs` `every_configuration_decodes_through_the_in_tree_decoder_within_its_bound` (new) runs the whole product under `cargo test -p tinker-pdf-filters` and prints how many configurations it ran; no reversible configuration differs by a sample, no irreversible one exceeds its computed bound, and a refusal is a failure rather than a skip. | ED-26.12 |
| ED-26.14 | measure | The encoder's speed rows in MS-04's encode group: a 5/3 and a 9/7 `jpx_encode` of one generated RGB image, sized to MS-04's 20 to 300 ms window, each with its baseline entry and the workflow's guard count. *Note:* No target is invented for them: a target is the owner's (H-77). | `crates/tinker-pdf/benches`, `crates/tinker-pdf/benches/baseline.json`, `.github/workflows/bench.yml`, `docs/verification.md` | `xtask/src/bench.rs` `the_committed_baseline_agrees_with_the_tree` holds the bench files, `bench.yml`'s guard counts and `baseline.json` to one list per group with both operations in the encode group, and `cargo bench -p tinker-pdf` reports their times, recorded in `docs/verification.md`. | ED-26.13, MS-04 |
| ED-26.15 | test | Fingerprint the encoder on the four targets: the SHA-256 of a 5/3 and a 9/7 encode of fixed generated images (tiled, layered, in a position progression) pinned in `determinism.rs`, entered in K-07's roster and grepped by the three native legs and `wasm-determinism`. *Note:* It lives in the facade's `determinism.rs` because that file already hashes with `tinker_pdf_crypto::sha2::sha256` and runs on all four legs, and the filters crate has no SHA-256. A hash that differs between targets is a determinism bug, never an edit. | `crates/tinker-pdf/tests/determinism.rs`, `xtask/src/determinism.rs` (new), `.github/workflows/ci.yml`, `docs/features/determinism.md` | `determinism.rs` `jpx_encoding_is_stable_across_targets` (new) passes on linux, windows, macos and `wasm32-wasip1`, in the default and `bundled-fonts` builds, each leg grepping it by name through the roster; the next CI run on `develop` is green on all four. | ED-26.13, K-07, MS-11 |

**Closes the row:** ED-26.5 encodes J.10.5's nine samples under J.10's parameters to J.10's published hundred bytes, the exit's first half, after ED-26.2 to ED-26.4 hold tier-1, the packet headers and the analysis to J.10's published intermediate values; ED-26.6 to ED-26.12 add the irreversible path, the component transforms, tiles, precincts, layers, the five progression orders and the JP2 file, and ED-26.13's sweep decodes every configuration through `jpx_decode`, exactly when reversible and within the bound computed in the test otherwise, the second half; ED-26.14 and ED-26.15 bring the speed rows and the fingerprint.

**The tree at `5297133`, against this row's text:**

- Nothing ships a JPEG 2000 writer, as Today says, but most of one exists as test fixtures that `rg -i 'jpx_encode|encode_jpx'` cannot find because none is named so: the tier-1 encoder `tier1::encoder::encode_code_block` (`crates/tinker-pdf-filters/src/jpx/tier1.rs:983-1145`, `cfg(test)`), the tag-tree encoder `encode_tag_tree_for_test` with its stuffing `BitWriter` (`jpx/tier2.rs:397-507`), the forward RCT written inside `the_inverse_rct_is_exact_over_the_forward_one` (`jpx/tests/colour.rs:140-142`), and the deliberately permissive header and JP2 writer `jpx/tests/writer.rs`.
- T.800 J.10 adjudicates the encoder stage by stage, not only end to end: J.10.4's coefficients (-26, -22, -30, -32, -19 and 1, 5, 1, 0, transcribed at `crates/tinker-pdf-filters/tests/jpx_annex_h.rs:183`, `:187`) are the 5/3 analysis of J.10.5's nine samples after the level shift (checked here by hand), J.10's two code-blocks carry sixteen and seven passes (`docs/verification.md:1512`), and B.10.7.1's NOTE 1 and B.10.2's worked example print packet-header bits (`jpx/tests/code_block_styles.rs:131-179`; `the_worked_tag_tree_example_reads_back_its_own_leaves`, `jpx/tests/tier2.rs:36`).
- The only MQ termination in the tree is T.88's FLUSH, which appends `FF AC` (`crates/tinker-pdf-filters/src/mq.rs:597-617`), and the test-only tier-1 encoder ends its code-blocks with it (`jpx/tier1.rs:1143`); J.10's published code-block bytes carry no such marker, so T.800's termination is new work (ED-26.2).
- The test writer's main header is SIZ, COD, QCD (`jpx/tests/writer.rs:156`), while J.10's codestream is SIZ, QCD, COD (`crates/tinker-pdf-filters/tests/jpx_annex_j.rs`, QCD at octal 00055 and COD at 00066), so the shipped writer takes J.10's order rather than the test writer's.

## ED-27 — JBIG2 symbol and text-region encoding

**JBIG2 symbol and text-region encoding** (lossless)

**Today.** generic regions only; symbol and text encoding a non-goal of the design until 9 October 2026 (`design/jbig2-symbol-text.md:84-99`)

**Exit criterion.** every output decodes to the input exactly through the in-tree decoder; and T.88 Annex H.1's symbol dictionary and text-region segments reproduced byte for byte by re-encoding, under the same parameters, the symbols and placements they decode to — whose composite page H.1 publishes, though it publishes no symbol bitmap of its own (the design doc's milestone 3 says so)

**Doc:** [design/jbig2-symbol-text.md](../design/jbig2-symbol-text.md) (amend) · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-27.1 | design | Amend `docs/design/jbig2-symbol-text.md` with the encoder: symbols as exact connected components, a lossless dictionary by height class with SDREFAGG 0 in arithmetic coding, text-region placement per 6.4.5, a generic region for what no symbol covers, and H.1's segments re-encoded from their decoded symbols as the byte-for-byte check. | `docs/design/jbig2-symbol-text.md` | The amended doc names ED-27.2 to ED-27.7's exit tests and the parameters H.1's segments are re-encoded under, and is reviewed. | — |
| ED-27.2 | build | Symbols from a bilevel image: its connected components extracted with their positions, identical components merged into one symbol. | `crates/tinker-pdf-filters/src/jbig2/encode_symbols.rs` (new), `crates/tinker-pdf-filters/src/jbig2.rs` | `encode_symbols` unit tests: generated bitmaps yield the components the test finds, and the symbols placed at their positions rebuild the bitmap exactly. | ED-27.1 |
| ED-27.3 | build | The symbol dictionary segment encoded with arithmetic coding, by height class. | `crates/tinker-pdf-filters/src/jbig2/encode_symbols.rs` (new), `crates/tinker-pdf-filters/tests/jbig2_annex_h.rs` (new) | `jbig2_annex_h.rs` (new) `h1s_symbol_dictionary_re_encodes_byte_for_byte`: H.1's decoded symbols under its parameters encode to H.1's segment; generated dictionaries decode through the in-tree decoder to their symbols exactly. | ED-27.2 |
| ED-27.4 | build | The text region segment encoded: symbol instances in strips with their S and T offsets and IDs, by 6.4.5's placement read backwards. | `crates/tinker-pdf-filters/src/jbig2/encode_symbols.rs` (new), `crates/tinker-pdf-filters/tests/jbig2_annex_h.rs` (new) | `h1s_text_region_re_encodes_byte_for_byte`: H.1's decoded placements encode to H.1's segment; generated regions decode to their placements exactly. | ED-27.3 |
| ED-27.5 | build | A whole page: page information, the dictionary, the text region and a generic region for the residue, written as a stream with its globals split for PDF. | `crates/tinker-pdf-filters/src/jbig2/encode_symbols.rs` (new), `crates/tinker-pdf-filters/src/lib.rs` | A unit test: generated scans with repeated glyphs and some residue encode and decode through the in-tree decoder to the input exactly. | ED-27.4 |
| ED-27.6 | build | The writer uses it: bilevel images recoded to symbol-coded JBIG2 when it is smaller, behind the recoding options and the CLI's `--bilevel`. | `crates/tinker-pdf/src/recode.rs`, `crates/tinker-pdf/tests/image_recode.rs`, `tools/tpdf/src/writing.rs`, `docs/features/writing.md` | `bilevel_images_symbol_code`: a recoded scan decodes to its samples exactly and the page draws the same pixels. | ED-27.5 |
| ED-27.7 | test | A fuzz target encoding arbitrary bitmaps and decoding them, the encoder's speed row and its fingerprint. | `fuzz/fuzz_targets/jbig2_encode.rs` (new), `fuzz/Cargo.toml`, `fuzz/README.md`, `docs/verification.md`, `crates/tinker-pdf/benches/engine.rs`, `crates/tinker-pdf/tests/determinism.rs` | The target asserts decode equals input and runs clean in the fuzz check, with the roster `docs/verification.md` pastes regenerated; the speed row is in the benchmark set; the fingerprint holds on every K-07 target. | ED-27.6, MS-04, K-07 |

**Closes the row:** ED-27.5 decodes every output to its input exactly and ED-27.3 and ED-27.4 reproduce H.1's dictionary and text-region segments byte for byte, which is the row's exit.

## ED-28 — JPEG encoder breadth

**JPEG encoder breadth**: optimised Huffman tables (Annex K.2), progressive mode, documented table presets for callers who want a named quality

**Today.** `tinker_pdf_filters::jpeg_encode` writes T.81's **baseline** process and only that — sequential DCT, Huffman, 8-bit, one SOF0 frame, one scan; grayscale or T.871's YCbCr with the JFIF APP0, 4:4:4 or 4:2:0 named by the caller and never inferred from the pixels; Annex K's tables, those halved, or the caller's own — with **no 1-to-100 quality number**, because every such scale in circulation is some program's private convention and none is in T.81, T.83 or T.871. Progressive and extended sequential, arithmetic, lossless, hierarchical, 12-bit, four-component CMYK/YCCK and K.2's optimised tables are excluded by name in the module header. The FDCT is held to A.3.3's equation recomputed in `f64` in the test, because no published DCT vector set is reachable (H-98)

**Exit criterion.** K.2's procedure recomputed in the test yields the emitted tables; progressive output decodes to the baseline output's samples exactly

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-28.1 | build | K.2's optimised Huffman tables: symbol counts gathered in a first pass and code lengths built, limited to 16, by K.2's procedure, written as the frame's tables. | `crates/tinker-pdf-filters/src/jpeg/encode.rs`, `crates/tinker-pdf-filters/src/jpeg.rs` | A unit test: K.2's procedure recomputed in the test yields the emitted tables, and the output decodes to the baseline output's samples exactly. | — |
| ED-28.2 | build | Progressive mode: an SOF2 frame with spectral selection and successive approximation scans by a default script (G.1). | `crates/tinker-pdf-filters/src/jpeg/encode.rs`, `crates/tinker-pdf-filters/src/jpeg.rs` | A unit test: progressive output decodes through the in-tree decoder to the baseline output's samples exactly, for grayscale and YCbCr at 4:4:4 and 4:2:0. | ED-28.1 |
| ED-28.3 | build | Optimised tables in progressive scans: K.2's procedure per scan over its own symbols, end-of-band runs included. | `crates/tinker-pdf-filters/src/jpeg/encode.rs` | A unit test: each scan's tables equal K.2 recomputed for it, and the output decodes to the baseline samples exactly. | ED-28.2 |
| ED-28.4 | doc | Named table presets: each a stated scaling of Annex K's tables, documented table by table in the feature doc and taken by name from the save options and the CLI, with no 1-to-100 scale. | `crates/tinker-pdf-filters/src/jpeg/encode.rs`, `crates/tinker-pdf/src/write.rs`, `docs/features/filters.md`, `crates/tinker-pdf/tests/determinism.rs` | A test holds each preset's tables to the ones the feature doc lists; a progressive encode with optimised tables is fingerprinted on every K-07 target. | ED-28.3, K-07 |

**Closes the row:** ED-28.1 and ED-28.3 emit tables equal to K.2 recomputed and ED-28.2 decodes progressive output to the baseline samples exactly, which is the row's exit; ED-28.4 adds the presets.

## ED-29 — Undo and redo

**Undo and redo**: a journal of named operations with undo, redo and inspection, a snapshot save, and the journal saved beside the file and reloaded when it matches

**Today.** transactions and checkpoints (`features/editing.md:274`)

**Exit criterion.** every editor operation undone restores the byte-identical previous save; a journal round-trips

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-29.1 | build | A journal over `DocumentEditor`: each public operation recorded as a named entry holding the objects it changed before and after, on the editor's transactions and checkpoints, with undo and redo. | `crates/tinker-pdf-cos/src/edit/journal.rs` (new), `crates/tinker-pdf-cos/src/edit/mod.rs`, `crates/tinker-pdf-cos/tests/journal.rs` (new), `docs/features/editing.md` | `journal.rs` (new) `every_operation_undoes_to_the_previous_save`: a table of every editor operation, each undone, saves byte-identically to the save before it, and redone, to the save after it. | — |
| ED-29.2 | build | Inspection and snapshots: the journal listed with each entry's name and the objects it touched, and a snapshot save that keeps the journal. | `crates/tinker-pdf-cos/src/edit/journal.rs` (new), `crates/tinker-pdf-cos/tests/journal.rs` (new) | `the_journal_is_inspected_and_snapshots_keep_it`: listing names every operation in order; after a snapshot save, undo still reaches the first state. | ED-29.1 |
| ED-29.3 | build | The journal saved beside the file with the identity of the bytes it belongs to, and reloaded only when they match. | `crates/tinker-pdf-cos/src/edit/journal.rs` (new), `crates/tinker-pdf-cos/tests/journal.rs` (new) | `a_journal_round_trips`: a saved journal reloads and undoes as before; against any other bytes it is refused by a typed error. | ED-29.2 |
| ED-29.4 | test | A property test of random operation sequences: every sequence undone to its start saves byte-identically to the original. | `crates/tinker-pdf-cos/tests/journal.rs` (new) | `random_sequences_undo_to_the_original` runs a fixed number of proptest cases and finds no failure; a counted injection that skips one entry's undo fires it. | ED-29.3 |

**Closes the row:** ED-29.1 restores the byte-identical previous save for every operation and ED-29.3 round-trips the journal, which is the row's exit.

## ED-30 — The low-level object API

**The low-level object API**: get, set and remove by path (`/Root/AcroForm/NeedAppearances`), deep copy and compare, a graft map that keeps sharing when copying objects between documents

**Today.** object-level get, put and allocate (`features/editing.md:11-88`)

**Exit criterion.** path operations held to fixtures; grafting two pages that share a font copies the font once

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-30.1 | build | Get, set and remove by path on `DocumentEditor`: names and array indices (`/Root/Pages/Kids/0`), references followed, a missing step a typed error. | `crates/tinker-pdf-cos/src/edit/paths.rs` (new), `crates/tinker-pdf-cos/src/edit/mod.rs`, `crates/tinker-pdf-cos/tests/object_paths.rs` (new), `docs/features/editing.md` | `object_paths.rs` (new) `path_operations_hold_to_fixtures`: a table of paths gets, sets and removes as expected, `/Root/AcroForm/NeedAppearances` among them. | — |
| ED-30.2 | build | Deep copy and comparison with cycles handled, and a graft map reused across copies between two documents so shared objects stay shared. | `crates/tinker-pdf-cos/src/edit/paths.rs` (new), `crates/tinker-pdf-cos/src/edit/page_ops.rs`, `crates/tinker-pdf-cos/tests/object_paths.rs` (new) | `grafting_two_pages_copies_their_font_once`: two pages sharing a font, grafted through one map, copy the font once; deep comparison tells equal and unequal graphs apart, cyclic ones included. | ED-30.1 |

**Closes the row:** ED-30.1 holds path operations to fixtures and ED-30.2 copies a shared font once, which is the row's exit.

## ED-31 — Revisions

**Revisions**: count incremental revisions with byte ranges, open as of revision N, extract revision N, diff two revisions by value, roll back

**Today.** incremental update with a byte-identical prefix; signatures' `modifications()` compares xref entries

**Exit criterion.** fixtures with known revisions; extracted revision N is byte-equal to its prefix; the value diff lists exactly the changed objects

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-31.1 | build | Revisions counted: each incremental section's byte range, from the cross-reference chain and each `%%EOF`. | `crates/tinker-pdf-cos/src/revisions.rs` (new), `crates/tinker-pdf-cos/src/lib.rs`, `crates/tinker-pdf-cos/tests/revisions.rs` (new) | `revisions.rs` (new) `revisions_are_counted_with_their_ranges`: builder fixtures saved incrementally n times report n revisions with the exact byte ranges the test recorded while saving. | — |
| ED-31.2 | build | Revision N extracted and opened: its prefix of the file, and a document opened as of it. | `crates/tinker-pdf-cos/src/revisions.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf-cos/tests/revisions.rs` (new) | `revision_n_is_its_prefix`: the extracted revision is byte-equal to its prefix, and opened as of it the document holds that revision's objects. | ED-31.1 |
| ED-31.3 | build | A value diff of two revisions: objects added, removed and changed by value, compared deeply through ED-30, beyond the cross-reference comparison signatures' `modifications()` makes. | `crates/tinker-pdf-cos/src/revisions.rs` (new), `crates/tinker-pdf-cos/tests/revisions.rs` (new) | `the_value_diff_lists_exactly_the_changed_objects`: fixtures with known edits, including an object rewritten with the same value, list exactly the objects whose values changed. | ED-31.2, ED-30 |
| ED-31.4 | build | Roll back to revision N, and `tpdf revisions` to list, extract, diff and roll back. | `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs`, `docs/features/editing.md` | A rolled-back save equals revision N's prefix; CLI tests run each subcommand over the revision fixtures. | ED-31.3 |

**Closes the row:** ED-31.1 to ED-31.3 count, extract and diff the known revisions exactly, and ED-31.4 rolls back, which is the row's exit.

## ED-32 — Check and repair on demand

**Check and repair on demand**: force a repair, a throw-on-repair mode so an edit is never lost to a late repair, a structure-tree check and repair (parents, cycles, spans, table attributes)

**Today.** the leniency ladder repairs during open with typed warnings

**Exit criterion.** a damaged fixture forced through repair before editing keeps every edit; structure-tree repairs held to fixtures with known defects; `tpdf repair` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-32.1 | build | Repair on demand and a refuse-on-repair mode: `repair.rs`'s ladder run when asked, before editing, and an open option that returns a typed error where it would repair late, so an edit is never lost to a later repair. | `crates/tinker-pdf-cos/src/repair.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/repair_on_demand.rs` (new), `docs/features/editing.md` | `repair_on_demand.rs` (new) `a_forced_repair_keeps_every_edit`: a damaged fixture repaired before editing keeps every edit through save and reopen; the refusing mode returns its error on it. | — |
| ED-32.2 | build | A structure-tree check and repair: parents that disagree with their children, cycles, spans that do not add up and table attributes, each found and fixed by name. | `crates/tinker-pdf/src/structure.rs`, `crates/tinker-pdf/tests/repair_on_demand.rs` (new) | `structure_repairs_hold_to_known_defects`: fixtures with each known defect are reported and repaired as the test expects, and a sound tree is left byte-identical. | — |
| ED-32.3 | build | `tpdf repair`, with the structure check as an option. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | `tpdf repair` runs over `testdata/` fixtures to the facade's output. | ED-32.1, ED-32.2 |

**Closes the row:** ED-32.1 keeps every edit through a forced repair, ED-32.2 repairs known structure defects and ED-32.3 runs `tpdf repair`, which is the row's exit.

## ED-33 — External streams internalised

**External streams internalised**: a stream with `/F` read through a host resource provider and written inside the file

**Today.** refused as `UntouchedImageReason::Filter` for an external `/F` (`features/writing.md:353`)

**Exit criterion.** an external-stream fixture saved self-contained renders identically

**Doc:** — · **Depends on:** FI-27

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-33.1 | build | External streams internalised on save: a stream's `/F` bytes read through FI-27's host resource provider and written inside the file, with `/FFilter` and `/FDecodeParms` becoming `/Filter` and `/DecodeParms`. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf/src/write.rs`, `crates/tinker-pdf/tests/external_streams.rs` (new), `docs/features/writing.md` | `external_streams.rs` (new) `an_external_stream_saves_self_contained`: the saved fixture has no `/F`, needs no provider, and renders identically; a missing resource is a typed refusal. | FI-27 |
| ED-33.2 | build | Image recoding takes an internalised stream, retiring the `UntouchedImageReason::Filter` refusal for an external `/F`. | `crates/tinker-pdf/src/recode.rs`, `crates/tinker-pdf/tests/external_streams.rs` (new) | `an_external_image_recodes`: an external image fixture recodes like an internal one; failing first on today's refusal. | ED-33.1 |

**Closes the row:** ED-33.1 saves the external-stream fixture self-contained, rendering identically, which is the row's exit.

## ED-34 — References to removed pages dropped on save

**References to removed pages dropped on save** (in `tinker-pdf-cos`)

**Today.** `keep_pages` keeps the catalog, and a garbage-collecting rewrite follows it, so a page an outline item, a named destination or a link still names stays in a split piece outside its page tree — page 6 of `outline-3level.pdf` split out carries five page objects (`a_page_the_outline_names_stays_in_a_piece_that_dropped_it`; `features/editing.md:848`); `delete_page` leaves the same references, and structure and `/OpenAction` too. `tpdf split` says it is not a redaction. A limit of the facade's rather than the CLI's, found when the CLI landed

**Exit criterion.** a page dropped by `keep_pages` or `delete_page` no longer reachable from anything the saved document keeps: references to it nulled or pruned on save, the pinned test inverted

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-34.1 | test | A reachability check over a saved document: no page object reachable from the trailer outside the page tree, used after every `keep_pages` and `delete_page` test. | `crates/tinker-pdf-cos/tests/removed_pages.rs` (new) | `removed_pages.rs` (new) `no_removed_page_is_reachable` runs over splits and deletions of the outline, destination, link, structure and `/OpenAction` fixtures and fails today, as the pinned test in `tools/tpdf/src/writing.rs` records. | — |
| ED-34.2 | fix | On save, outline items, named destinations and links naming a removed page pruned (an outline item's children lifted to its parent). | `crates/tinker-pdf-cos/src/edit/save.rs`, `crates/tinker-pdf-cos/src/edit/page_ops.rs`, `crates/tinker-pdf-cos/tests/removed_pages.rs` (new), `tools/tpdf/src/writing.rs` | The outline, destination and link cases of `no_removed_page_is_reachable` pass; `a_page_the_outline_names_stays_in_a_piece_that_dropped_it` is inverted, page 6 of `outline-3level.pdf` split out carrying one page object. | ED-34.1 |
| ED-34.3 | fix | On save, structure elements whose `/Pg` is a removed page pruned with their parent-tree entries, and an `/OpenAction` naming a removed page removed. | `crates/tinker-pdf-cos/src/edit/save.rs`, `crates/tinker-pdf-cos/tests/removed_pages.rs` (new), `docs/features/editing.md` | Every case of `no_removed_page_is_reachable` passes; a tagged split still passes the PDF/UA checks it passed before; the feature doc's limit is removed. | ED-34.2 |

**Closes the row:** ED-34.2 and ED-34.3 leave no removed page reachable and invert the pinned test, which is the row's exit.

## ED-35 — Type 3 glyph procedures rewritten under redaction

**Type 3 glyph procedures rewritten under redaction** (redaction clause (a))

**Today.** Redaction cuts a Type 3 run in its own glyph space and a glyph at the use whose procedure draws under a rectangle, measured in the enclosing scope and in the font's own `/Resources` (`glyph_procedures`). **Glyph-procedure streams are not rewritten** (`features/editing.md:836`): the exit asked for them rewritten; what was built instead removes the *use* and leaves the procedure, which `subset::apply` — the default save — empties once nothing shown runs it (`subset.rs`'s `type3_fonts`, which walks tiling cells and mask groups and counts a glyph whose procedure this engine could not run), so a procedure some uncovered use still runs keeps the covered words in `/CharProcs`; and what a procedure draws — a form it invokes, a soft mask's group it sets — is not cut either, and emptying the procedure removes neither from the file (a group is measured at every use, one the glyph's own box removed included, and named `PatternOrMask` when it shows text under a rectangle there, whatever removed the use, `a_glyph_whose_procedure_masks_text_under_a_rectangle_is_removed` and `a_covered_glyph_whose_procedure_masks_text_under_a_rectangle_names_the_group` — or when the use's `MAX_PLACEMENTS`-stream budget cut its measurement short, `a_group_after_a_naming_that_spent_the_budget_is_named`, what the budget cut short anywhere else naming the font `UnboundedProcedure`, `a_covered_use_whose_measurement_ran_out_says_so`; a form is not named). That substitute was the implementing lane's choice, argued in `cut_stream`'s doc and the refusal table, and is the owner's to accept or refuse (H-11); until then this criterion is open

**Exit criterion.** glyph-procedure streams rewritten, so the covered words are absent from every procedure stream an uncovered use still runs, and what a procedure draws under a rectangle is cut; or the owner's ruling that removal at the use with `subset::apply`'s emptying suffices

**Doc:** — · **Depends on:** H-11

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-35.1 | doc | Record H-11's ruling in `docs/rulings.md` and the feature doc: if the owner accepts removal at the use with `subset::apply`'s emptying, the row closes here; if the owner requires procedures rewritten, ED-35.2 to ED-35.4 follow. | `docs/rulings.md`, `docs/features/editing.md` | The ruling is recorded with its date; the refusal table's row for clause (a) says which branch holds. | H-11 |
| ED-35.2 | build | Glyph-procedure streams rewritten under redaction through ED-04's pipeline: a procedure an uncovered use still runs is cut in the font's glyph space so the covered words leave every procedure stream. *Note:* Only if H-11 requires procedures rewritten. | `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/tests/redact_type3.rs` (new) | `redact_type3.rs` (new) `covered_words_leave_every_procedure_stream`: in fixtures where a covered and an uncovered use share a glyph, the covered words are absent from every procedure stream, and the uncovered use draws as before. | ED-35.1, ED-04 |
| ED-35.3 | build | What a procedure draws under a rectangle cut: a form it invokes and a soft mask group it sets rewritten per use, so the namings `PatternOrMask` and `UnboundedProcedure` remain only where the measurement is genuinely cut short. *Note:* Only if H-11 requires procedures rewritten. | `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/tests/redact_type3.rs` (new) | `what_a_procedure_draws_is_cut`: a procedure's form and mask group under a rectangle lose the covered content; the existing namings tests still pass where the budget runs out. | ED-35.2 |
| ED-35.4 | test | The row's fixtures in the redaction suite and a fingerprint of one rewritten Type 3 page. *Note:* Only if H-11 requires procedures rewritten. | `crates/tinker-pdf/tests/redact_type3.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | Every glyph-procedure fixture of the refusal table passes with the procedure rewritten; the fingerprint holds on every K-07 target. | ED-35.3, K-07 |

**Closes the row:** ED-35.1 closes the row on the owner's ruling that removal at the use suffices, or ED-35.2 to ED-35.4 rewrite the procedures and cut what they draw, the row's two exits.

## ED-36a — Image redaction modes

**Image redaction modes**: none; the whole image removed; the pixels under a rectangle blanked and the image recoded losslessly; and removed unless invisible, by a clip-aware estimate of whether the covered part is drawn

**Today.** the whole image is scrubbed (`features/editing.md:822`). The image encoders exist — `tinker-pdf-filters` writes PNG, CCITT G4 and JBIG2 generic regions (15 September 2026) and baseline JPEG (16 September), and the writer calls them only when a save's `SaveOptions::images` names one — so this waits on the decision of when to choose a codec rather than on a codec (H-20)

**Exit criterion.** the blanked region's samples are the fill and every other sample is unchanged; each mode's survivors match fixtures with known clips; `tpdf redact --images <mode>` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** H-20

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-36a.1 | build | Image redaction by blanking: the samples under a rectangle set to the fill and the image recoded losslessly by the codec H-20 decides. | `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/src/recode.rs`, `crates/tinker-pdf/tests/redact_images.rs` (new), `docs/features/editing.md` | `redact_images.rs` (new) `blanked_samples_are_the_fill_and_the_rest_unchanged`: under the rectangle every sample is the fill and every other sample equals the original's. | H-20 |
| ED-36a.2 | build | The other image modes: none, whole image removed, and removed unless invisible by a clip-aware estimate of whether the covered part is drawn. | `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/tests/redact_images.rs` (new) | `each_image_mode_keeps_its_survivors`: fixtures with known clips keep exactly the images each mode should. | ED-36a.1 |
| ED-36a.3 | build | `tpdf redact --images <mode>`. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | `tpdf redact --images` runs each mode over `testdata/` fixtures to the facade's output. | ED-36a.2, ED-36b |

**Closes the row:** ED-36a.1 blanks only the covered samples, ED-36a.2 keeps each mode's survivors and ED-36a.3 runs `tpdf redact --images`, which is the row's exit.

## ED-36b — Redaction modes for line art and text

**Redaction modes for line art and text**: line art none, remove if covered, remove if touched; text none (marks only) beside the default removal

**Today.** absent — the owner's parity decision of 9 October 2026; redaction removes text and images and leaves line art: the redaction module "does not follow paths" (`features/editing.md:824`)

**Exit criterion.** each mode held to fixtures with known survivors; `tpdf redact --line-art <mode>` and `--text none` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-36b.1 | build | Line-art redaction modes: none, remove a path the rectangles cover wholly, remove a path they touch, by the paths' geometry from ED-01's objects. | `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/tests/redact_line_art.rs` (new), `docs/features/editing.md` | `redact_line_art.rs` (new) `each_line_art_mode_keeps_its_survivors`: fixtures with known paths keep exactly the survivors each mode should. | ED-01 |
| ED-36b.2 | build | Text mode none: the redaction marks drawn and the text left, beside the default removal. | `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/tests/redact_line_art.rs` (new) | `text_none_marks_only`: the text extracts as before and the marks are drawn. | ED-36b.1 |
| ED-36b.3 | build | `tpdf redact`, rectangles from arguments or a file, with `--line-art <mode>` and `--text none`. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | `tpdf redact` runs each mode over `testdata/` fixtures to the facade's output. | ED-36b.2 |

**Closes the row:** ED-36b.1 and ED-36b.2 hold each mode to known survivors and ED-36b.3 runs it from the CLI, which is the row's exit.

## ED-36c — A flatten-to-image redaction mode

**A flatten-to-image redaction mode** that leaves no vector or text residue

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'flatten_to_image' crates tools` finds nothing

**Exit criterion.** the output page carries one image and no text, and its render equals the redacted render; `tpdf redact --flatten` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-36c.1 | build | Flatten to image: the redacted page rendered at a stated resolution and its content replaced by that one image. | `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/tests/redact_flatten.rs` (new), `docs/features/editing.md` | `redact_flatten.rs` (new) `a_flattened_page_is_one_image_and_no_text`: the page carries one image and no text or path, and renders equal to the redacted render at that resolution. | — |
| ED-36c.2 | build | `tpdf redact --flatten` and a fingerprint of one flattened page. | `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/determinism.rs` | `tpdf redact --flatten` runs over `testdata/` fixtures to the facade's output; the fingerprint holds on every K-07 target. | ED-36c.1, ED-36b, K-07 |

**Closes the row:** ED-36c.1 leaves one image and no text, rendering as the redacted page, and ED-36c.2 runs `tpdf redact --flatten`, which is the row's exit.

## ED-36d — Mark, review, apply

**Mark, review, apply**: `/Redact` annotations created, listed and applied, with overlay text, codes, `/Repeat`, fill and cross-out

**Today.** redaction applies caller rectangles directly

**Exit criterion.** marks round-trip; application equals direct application with the same rectangles plus the overlay; `tpdf redact` by area and by search, marking and applying runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** AN-06

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-36d.1 | build | `/Redact` annotations created and listed through AN-06's creation: quads, overlay text, `/Repeat`, fill colour, cross-out appearance and redaction codes. | `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/src/annotations.rs`, `crates/tinker-pdf/tests/redact_marks.rs` (new), `docs/features/editing.md` | `redact_marks.rs` (new) `marks_round_trip`: every property written reads back as written and the saved file validates strictly. | AN-06 |
| ED-36d.2 | build | Applying marks: each `/Redact` applied as the direct redaction of its areas, then its overlay drawn (fill and overlay text, repeated where `/Repeat` asks), and the mark removed. | `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/tests/redact_marks.rs` (new) | `applying_marks_equals_direct_redaction_plus_the_overlay`: the applied page's content and render equal direct redaction with the same rectangles plus the overlay the test draws. | ED-36d.1 |
| ED-36d.3 | build | `tpdf redact` by area and by search, marking (writing `/Redact` annotations) and applying. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | CLI tests mark by area and by search, then apply, over `testdata/` fixtures to the facade's output. | ED-36d.2, ED-36b |

**Closes the row:** ED-36d.1 round-trips marks, ED-36d.2 applies them as direct redaction plus the overlay, and ED-36d.3 runs it from the CLI, which is the row's exit.

## ED-36e — No residue outside the content

**No residue outside the content**: structure elements, `/ActualText`, an annotation's own `/Contents`, field values, outline titles, named destinations and attachments that reference redacted content scrubbed; links and annotations in the area removed

**Today.** an annotation's own text is left when its appearance is cut (`features/editing.md:838`)

**Exit criterion.** fixtures with redacted words planted in each place find none after application (text search and raw-stream search); `tpdf redact`, by default runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-36e.1 | test | A residue search after redaction: the redacted words sought by text search and in every decoded stream and every string of the saved file, over fixtures planting them in each place the row names. | `crates/tinker-pdf/tests/redact_residue.rs` (new) | `redact_residue.rs` (new) `no_residue_anywhere` runs over each planted place and fails today for the places not yet scrubbed, naming each. | — |
| ED-36e.2 | fix | Structure residue scrubbed: `/ActualText`, `/Alt` and `/E` of elements whose content was redacted, and elements left with no content removed with their parent-tree entries. | `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/src/structure.rs`, `crates/tinker-pdf/tests/redact_residue.rs` (new) | The structure cases of `no_residue_anywhere` pass; a tagged fixture still passes the PDF/UA checks it passed. | ED-36e.1 |
| ED-36e.3 | fix | Annotation and field residue: links and annotations in a redacted area removed, an annotation's own `/Contents` scrubbed when its appearance is cut, and field values holding redacted words cleared. | `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/tests/redact_residue.rs` (new) | The annotation and field cases of `no_residue_anywhere` pass; annotations outside the areas are unchanged. | ED-36e.1 |
| ED-36e.4 | fix | Document-level residue: outline titles, named destinations and attachments that reference redacted content scrubbed. | `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/tests/redact_residue.rs` (new) | Every case of `no_residue_anywhere` passes. | ED-36e.2, ED-36e.3 |
| ED-36e.5 | build | The scrub on by default in `tpdf redact`, and a fingerprint of one scrubbed document. | `tools/tpdf/src/writing.rs`, `crates/tinker-pdf/tests/determinism.rs`, `docs/features/editing.md` | `tpdf redact` with no option scrubs every place over `testdata/` fixtures; the fingerprint holds on every K-07 target; the feature doc's residue limit is removed. | ED-36e.4, ED-36b, K-07 |

**Closes the row:** ED-36e.2 to ED-36e.4 leave none of the planted words by text or raw-stream search, and ED-36e.5 makes it `tpdf redact`'s default, which is the row's exit.

## ED-36f — Remove only invisible text

**Remove only invisible text** (render mode 3), the redaction mode for stripping an old OCR layer

**Today.** absent — the owner's parity decision of 9 October 2026; invisible text is shown to the device like any other (`invisible_text_is_still_shown_to_the_device`, `crates/tinker-pdf-content/src/interpret.rs:2378`) and no redaction mode keys on it

**Exit criterion.** fixtures with visible and invisible runs keep exactly the visible; `tpdf redact --invisible-text` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-36f.1 | build | A redaction mode removing only text drawn in render mode 3, on a whole page or inside rectangles. | `crates/tinker-pdf/src/redact.rs`, `crates/tinker-pdf/tests/redact_invisible.rs` (new), `docs/features/editing.md` | `redact_invisible.rs` (new) `only_invisible_text_goes`: fixtures with visible and invisible runs, interleaved in one text object, keep exactly the visible; the render is unchanged. | — |
| ED-36f.2 | build | `tpdf redact --invisible-text`. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | `tpdf redact --invisible-text` runs over `testdata/` fixtures to the facade's output. | ED-36f.1, ED-36b |

**Closes the row:** ED-36f.1 keeps exactly the visible text and ED-36f.2 runs `tpdf redact --invisible-text`, which is the row's exit.

## ED-37 — Write-option parity

**Write-option parity**: decompress everything, recompress already-filtered streams (Flate at a higher effort, Brotli), compression effort, compress only fonts or only images, ASCII-hex armour, pretty and compact object syntax, keep metadata untouched, continue on error, regenerate or keep `/ID`

**Today.** a filtered stream is "handed through untouched" (`features/writing.md:344`); `WriteOptions` carries mode, linearize, version, object streams, compress, encryption, garbage collection, deduplication

**Exit criterion.** each option held to fixtures; outputs validate strictly and render identically; `tpdf linearize` and `tpdf clean` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** FO-18

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-37.1 | build | Stream re-filtering on save: decompress everything, or add ASCII-hex armour, by decoding each stream's chain and writing the chosen one. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf/src/write.rs`, `crates/tinker-pdf/tests/write_options.rs` (new), `docs/features/writing.md` | `write_options.rs` (new) `decompress_and_armour`: each option over every committed fixture validates strictly and renders identically; no output stream keeps a filter under decompression. | — |
| ED-37.2 | build | Already-filtered streams recompressed with Flate at a stated effort, and the compression effort option for every stream the writer deflates. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf/tests/write_options.rs` (new) | `recompression_and_effort`: outputs validate and render identically, and the sizes at each effort are recorded in the test's output. | ED-37.1 |
| ED-37.3 | build | Recompression with Brotli through FO-18's encoder. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf/tests/write_options.rs` (new) | `brotli_recompression`: outputs validate and render identically. | ED-37.2, FO-18 |
| ED-37.4 | build | Compress only fonts or only images, by each stream's role. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf/tests/write_options.rs` (new) | `compress_by_role`: only the chosen role's streams change filter, and outputs render identically. | ED-37.2 |
| ED-37.5 | build | Pretty and compact object syntax. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf/tests/write_options.rs` (new) | `pretty_and_compact_syntax`: both forms re-read to the same objects, validate strictly and render identically; compact is never larger than the default. | — |
| ED-37.6 | build | Metadata kept untouched, and `/ID` regenerated or kept. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf/tests/write_options.rs` (new) | `metadata_untouched_and_id_choices`: the XMP stream and Info dictionary are byte-identical when kept; `/ID` is kept or regenerated as asked. | — |
| ED-37.7 | build | Continue on error: a stream that fails to decode is written as it was, with a typed warning, instead of failing the save. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf/tests/write_options.rs` (new) | `a_bad_stream_is_kept_and_warned`: a fixture with an undecodable stream saves under decompression with that stream unchanged and the warning raised. | ED-37.1 |
| ED-37.8 | build | `tpdf linearize`, and the options on it and on `tpdf clean`. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | CLI tests run each option through both commands over `testdata/` fixtures to the facade's output. | ED-37.3, ED-37.4, ED-37.5, ED-37.6, ED-37.7, ED-04 |

**Closes the row:** ED-37.1 to ED-37.7 hold each option to fixtures, validating strictly and rendering identically, and ED-37.8 runs them from `tpdf linearize` and `tpdf clean`, which is the row's exit.

## ED-38 — Streaming write

**Streaming write**: finished pages flushed to the sink; memory proportional to the largest object

**Today.** a non-goal of the streaming design until 9 October 2026 (`design/streaming-open.md:47-66`)

**Exit criterion.** a 10 000-page generated document written with peak memory within an MS-05 band independent of page count

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-38.1 | design | Amend `docs/design/streaming-open.md` with the streaming writer: object numbers reserved ahead, objects written to a sink as they finish, the page tree and shared resources written last, and what cannot stream (object streams, linearization, cross-page deduplication). | `docs/design/streaming-open.md` | The amended doc names ED-38.2 to ED-38.4's exit tests and is reviewed. | — |
| ED-38.2 | build | A sink writer in `tinker-pdf-cos`: objects written as handed over, offsets recorded, the cross-reference table and trailer at the end. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf-cos/tests/streaming_write.rs` (new) | `streaming_write.rs` (new) `the_sink_writer_matches_the_whole_writer`: for one object order its bytes equal the whole-document writer's. | ED-38.1 |
| ED-38.3 | build | The builder's streaming mode: each finished page's own objects flushed, shared resources and the subset fonts written once every page is done. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf-cos/tests/streaming_write.rs` (new), `docs/features/creation.md` | `streamed_documents_match`: generated documents built streaming validate strictly and render identically to their whole builds. | ED-38.2 |
| ED-38.4 | measure | Peak memory of a streamed 10 000-page generated document, held in an MS-05 band that does not grow with page count. | `crates/tinker-pdf-cos/tests/streaming_write.rs` (new) | The 10 000-page write's peak lies in the same MS-05 band as the 1 000-page write's, and the band is committed. | ED-38.3, MS-05 |

**Closes the row:** ED-38.4 writes a 10 000-page document with peak memory in an MS-05 band independent of page count, on ED-38.2 and ED-38.3's streaming, which is the row's exit.

## ED-39 — Attachment parity

**Attachment parity**: checksum verification, update, delete, extraction from the CLI, file-attachment annotations embedded and extracted

**Today.** attachments listed and written; `tpdf attach` adds one

**Exit criterion.** each operation round-trips; a corrupted checksum is reported by a typed warning; `tpdf attachments list`, `add`, `remove` and `extract` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-39.1 | build | Embedded files verified, updated and deleted: `/CheckSum` checked against the bytes with a typed warning, a file replaced, a file removed from the name tree. | `crates/tinker-pdf-cos/src/edit/docops.rs`, `crates/tinker-pdf-cos/src/outline.rs`, `crates/tinker-pdf/tests/attachments.rs` (new), `docs/features/editing.md` | `attachments.rs` (new) `attachments_update_delete_and_verify`: each operation round-trips; a corrupted checksum is reported by the warning. | — |
| ED-39.2 | build | File-attachment annotations embedded and extracted. | `crates/tinker-pdf-cos/src/edit/annotations.rs`, `crates/tinker-pdf/src/annotations.rs`, `crates/tinker-pdf/tests/attachments.rs` (new) | `file_attachment_annotations_round_trip`: an embedded annotation's file extracts byte for byte and the file validates strictly. | ED-39.1 |
| ED-39.3 | build | `tpdf attachments list`, `add`, `remove` and `extract`, beside the existing `attach`. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | CLI tests run each subcommand over `testdata/` fixtures and read the result back through the facade. | ED-39.2 |

**Closes the row:** ED-39.1 and ED-39.2 round-trip each operation with the checksum warning, and ED-39.3 runs the four subcommands, which is the row's exit.

## ED-40 — Layer authoring breadth

**Layer authoring breadth**: OCMDs with `/VE`, `/Configs`, the UI `/Order`, `/RBGroups`, optional content on annotations, images and existing content

**Today.** `add_layer`, `PageBuilder::optional`, visibility toggled in `/D`

**Exit criterion.** each authored structure re-read and rendered under each configuration as RC-14 decides; `tpdf layers` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** RC-14

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-40.1 | build | Authoring OCMDs with `/VE`, alternate `/Configs`, the UI `/Order` and `/RBGroups`, in the builder and the editor. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf-cos/src/edit/layers.rs`, `crates/tinker-pdf/tests/layer_authoring.rs` (new), `docs/features/creation.md` | `layer_authoring.rs` (new) `layer_structures_re_read_as_written`: each structure re-reads as written and validates strictly. | — |
| ED-40.2 | build | Optional content put on annotations, images and existing content (its objects wrapped in marked content naming the group). | `crates/tinker-pdf-cos/src/edit/layers.rs`, `crates/tinker-pdf/tests/layer_authoring.rs` (new) | `layers_on_annotations_images_and_content`: each authored structure renders under each configuration as RC-14 decides. | ED-40.1, RC-14, ED-01 |
| ED-40.3 | build | `tpdf layers`: list, add, set visibility and choose a configuration. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | CLI tests run each subcommand over `testdata/` fixtures to the facade's output. | ED-40.2 |

**Closes the row:** ED-40.1 and ED-40.2 re-read and render each structure under each configuration as RC-14 decides, and ED-40.3 runs `tpdf layers`, which is the row's exit.

## ED-41 — Images to PDF without re-encoding

**Images to PDF without re-encoding**: JPEG, JPEG 2000, CCITT and JBIG2 passed through; PNG through Flate with predictors (16-bit, palette, alpha to soft mask); multi-page TIFF to pages; resolution to page size; EXIF orientation; PDF/A output

**Today.** the builder deflates every raster it is handed (`features/creation.md:382`); JPEG and PNG IDAT pass through

**Exit criterion.** the image stream bytes equal the input's coded bytes; decoded samples equal the source's

**Doc:** — · **Depends on:** FI-14

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-41.1 | build | JPEG 2000, CCITT (from TIFF strips) and JBIG2 images passed through to image streams with their coded bytes, beside today's JPEG and PNG IDAT passthrough. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf-cos/src/raster_embed.rs`, `crates/tinker-pdf/tests/image_passthrough.rs` (new), `docs/features/creation.md` | `image_passthrough.rs` (new) `coded_bytes_pass_through`: each image stream's bytes equal the input's coded bytes and its decoded samples equal the source's. | — |
| ED-41.2 | build | PNG through Flate with predictors: 16-bit and palette images passed through, and alpha split into a soft mask. | `crates/tinker-pdf-cos/src/png_embed.rs`, `crates/tinker-pdf/tests/image_passthrough.rs` (new) | `png_variants_embed`: decoded samples, soft masks included, equal the source's for each variant. | ED-41.1 |
| ED-41.3 | build | Multi-page TIFF to pages, resolution to page size, and orientation applied as FI-14 reads it. | `crates/tinker-pdf-cos/src/tiff_embed.rs`, `crates/tinker-pdf/tests/image_passthrough.rs` (new) | `tiff_pages_sizes_and_orientation`: a three-page TIFF yields three pages sized from each page's resolution, each oriented as FI-14's tests expect. | ED-41.1, FI-14 |
| ED-41.4 | build | PDF/A output for images to PDF, and a fingerprint of one converted document. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/image_passthrough.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | `images_to_pdf_a_validate`: outputs pass the engine's PDF/A validation for the chosen part; the fingerprint holds on every K-07 target. | ED-41.2, ED-41.3, K-07 |

**Closes the row:** ED-41.1 and ED-41.2 keep coded bytes and decoded samples equal to the input's, ED-41.3 maps TIFF pages, resolution and orientation, and ED-41.4 adds PDF/A output, which covers the row's exit.

## ED-42 — Convert any opened document to PDF and insert its pages

**Convert any opened document to PDF and insert its pages**: EPUB, XPS, CBZ, SVG, HTML, Markdown, FB2 and images inserted into an existing PDF with links

**Today.** each format is synthesised into a real PDF on open; nothing inserts one into another

**Exit criterion.** inserted pages render as the opened document's pages; links retargeted; `tpdf convert --insert` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-13

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-42.1 | build | Any opened document's pages inserted into an existing PDF: the PDF each format is synthesised into on open, merged at an index through ED-13's merge, its links retargeted. | `crates/tinker-pdf/src/merge.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/insert_converted.rs` (new), `docs/features/editing.md` | `insert_converted.rs` (new) `inserted_pages_render_as_the_opened_document`: for EPUB, XPS, CBZ, SVG, HTML, Markdown, FB2 and an image, the inserted pages render as the opened document's pages and its internal links land on the inserted pages. | ED-13 |
| ED-42.2 | build | `tpdf convert --insert` and a fingerprint of one document with inserted pages. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs`, `crates/tinker-pdf/tests/determinism.rs` | `tpdf convert --insert` runs over `testdata/` fixtures of each format to the facade's output; the fingerprint holds on every K-07 target. | ED-42.1, K-07 |

**Closes the row:** ED-42.1 renders inserted pages as the opened document's with links retargeted and ED-42.2 runs `tpdf convert --insert`, which is the row's exit.

## ED-43 — HTML into a rectangle of an existing page

**HTML into a rectangle of an existing page**, with scaling to fit, rotation, layer and opacity, a positions callback and overflow reporting; a DOM API to change the HTML before layout — create, find, insert, remove and clone elements and text, and set attributes

**Today.** `FromHtml` makes whole documents (`features/creation.md:13-45`)

**Exit criterion.** the placed content renders as the same HTML laid out at that size; overflow reported exactly when content exceeds the box; a DOM edit lays out as the same HTML written with the edit

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-43.1 | build | HTML laid out at a rectangle's size by `FromHtml`'s pipeline and placed on an existing page as a form, with scaling to fit, rotation, a layer and opacity. | `crates/tinker-pdf/src/html.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/html_in_a_box.rs` (new), `docs/features/creation.md` | `html_in_a_box.rs` (new) `placed_html_renders_as_its_own_layout`: the placed content renders as the same HTML laid out at that size, under each placement option. | — |
| ED-43.2 | build | Overflow reported exactly when the content exceeds the box, and a callback given each element's laid-out rectangle. | `crates/tinker-pdf/src/html.rs`, `crates/tinker-pdf/tests/html_in_a_box.rs` (new) | `overflow_is_reported_exactly`: content sized to fit reports none and one line more reports it; the callback's rectangles equal the layout's. | ED-43.1 |
| ED-43.3 | build | A DOM API on the parsed tree before layout: create, find, insert, remove and clone elements and text, and set attributes. | `crates/tinker-pdf-layout/src/html/mod.rs` (new), `crates/tinker-pdf/src/html.rs`, `crates/tinker-pdf/tests/html_in_a_box.rs` (new) | `dom_edits_lay_out_as_the_edited_html`: each edit lays out as the same HTML written with the edit. | ED-43.1 |

**Closes the row:** ED-43.1 renders placed HTML as its own layout, ED-43.2 reports overflow exactly and ED-43.3's edits lay out as the edited HTML, which is the row's exit.

## ED-44 — A programmatic layout API

**A programmatic layout API**: paragraph, division, table, list, image and break with pagination, repeating table headers and footers, row spans, keep-together and keep-with-next, floats, columns, justification and hyphenation

**Today.** "layout on the builder's own methods — by design" (`features/creation.md:385`)

**Exit criterion.** layouts built through the API equal the same content laid out from HTML (reftest pairs); a 10 000-row table streams within an MS-05 band

**Doc:** `design/layout-api.md` (owed) · **Depends on:** FT-15

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-44.1 | design | Create `docs/design/layout-api.md`: the API's elements and how each maps onto the layout crate's box tree (so the API and HTML share one engine), pagination, repeating headers and footers, row spans, keep rules, floats, columns, justification and hyphenation, the reftest method pairing each API layout with HTML, and a large table's streaming. | `docs/design/layout-api.md` (new) | The doc exists in the design-doc order, names ED-44.2 to ED-44.8's exit tests, and is reviewed. | — |
| ED-44.2 | build | Paragraph, division, image and break built into the layout crate's box tree. | `crates/tinker-pdf/src/layout_api.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/layout_api.rs` (new) | `layout_api.rs` (new) `blocks_equal_their_html`: each reftest pair (API, HTML) renders identically. | ED-44.1 |
| ED-44.3 | build | Lists and tables, row and column spans included. | `crates/tinker-pdf/src/layout_api.rs` (new), `crates/tinker-pdf/tests/layout_api.rs` (new) | `lists_and_tables_equal_their_html`: each reftest pair renders identically. | ED-44.2 |
| ED-44.4 | build | Pagination: repeating table headers and footers, keep-together and keep-with-next. | `crates/tinker-pdf/src/layout_api.rs` (new), `crates/tinker-pdf/tests/layout_api.rs` (new) | `pagination_equals_its_html`: reftest pairs breaking across pages render identically page by page. | ED-44.3 |
| ED-44.5 | build | Floats and columns. | `crates/tinker-pdf/src/layout_api.rs` (new), `crates/tinker-pdf/tests/layout_api.rs` (new) | `floats_and_columns_equal_their_html`: each reftest pair renders identically. | ED-44.2 |
| ED-44.6 | build | Justification and hyphenation, hyphenation through FT-15. | `crates/tinker-pdf/src/layout_api.rs` (new), `crates/tinker-pdf/tests/layout_api.rs` (new) | `justified_and_hyphenated_text_equals_its_html`: each reftest pair renders identically, in both builds. | ED-44.2, FT-15 |
| ED-44.7 | measure | A 10 000-row table streamed through the layout API onto ED-38's streaming writer, its peak memory held in an MS-05 band. | `crates/tinker-pdf/src/layout_api.rs` (new), `crates/tinker-pdf/tests/layout_api.rs` (new) | The 10 000-row table's peak lies in the same committed MS-05 band as a 1 000-row table's. | ED-44.4, ED-38, MS-05 |
| ED-44.8 | measure | The API's speed row and fingerprint, and its feature doc. | `crates/tinker-pdf/benches/engine.rs`, `crates/tinker-pdf/tests/determinism.rs`, `docs/features/creation.md` | A 20-page generated layout runs in the benchmark set; its bytes are fingerprinted on every K-07 target. | ED-44.5, ED-44.6, MS-04, K-07 |

**Closes the row:** ED-44.2 to ED-44.6 make every API layout equal its HTML reftest pair and ED-44.7 streams a 10 000-row table within an MS-05 band, which is the row's exit.

## ED-45 — Shaped text on the builder

**Shaped text on the builder**: text in any font and script shaped, embedded and subset in one call; simple fonts for Latin, Greek and Cyrillic encodings; a non-embedded CJK font with a predefined CMap; a text box filled with alignment and overflow reporting

**Today.** `text` is one byte per character; shaping is `glyph_run`'s (`features/creation.md:383`)

**Exit criterion.** extracted text equals the input in logical order for every committed shaping fixture's script; overflow reported exactly

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-45.1 | build | One builder call: text in an OpenType font shaped by the shaper, embedded as a CID font with `Identity-H`, subset, with the `/ToUnicode` `shaping.rs`'s rules give. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/src/shaping.rs`, `crates/tinker-pdf/tests/builder_text.rs` (new), `docs/features/creation.md` | `builder_text.rs` (new) `shaped_text_extracts_in_logical_order`: for every committed shaping fixture's script, the extracted text equals the input in logical order, in both builds. | — |
| ED-45.2 | build | Simple fonts when the text fits one: Latin, Greek and Cyrillic encodings with `/Differences`. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/builder_text.rs` (new) | `simple_fonts_for_small_alphabets`: Latin, Greek and Cyrillic text writes a simple font, extracts as the input, and is smaller than its CID twin. | ED-45.1 |
| ED-45.3 | build | A non-embedded CJK font with a predefined CMap, named by registry, ordering and supplement. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/builder_text.rs` (new) | `cjk_with_a_predefined_cmap`: Japanese, Chinese and Korean text extracts as the input through the vendored CMaps and validates strictly. | ED-45.1 |
| ED-45.4 | build | A text box filled with alignment (left, right, centred, justified) and overflow reported exactly. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/builder_text.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | `text_boxes_align_and_report_overflow`: line positions equal the test's for each alignment; overflow is reported exactly when the text exceeds the box; one box's bytes are fingerprinted on every K-07 target. | ED-45.1, K-07 |

**Closes the row:** ED-45.1 extracts every committed script in logical order and ED-45.4 reports overflow exactly, which is the row's exit; ED-45.2 and ED-45.3 add simple and CJK fonts.

## ED-46 — Drawing helpers

**Drawing helpers**: lines, polylines, rectangles, circles, ellipses, sectors, Bézier curves, zigzags, with stroke and fill options

**Today.** `fill_rect` and raw content

**Exit criterion.** each helper's path equals the geometry computed in the test

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-46.1 | build | Drawing helpers on the builder: lines, polylines, rectangles, circles and ellipses (Bézier arcs with the stated control-point constant), sectors, Bézier curves and zigzags, with stroke and fill options. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf-cos/tests/drawing_helpers.rs` (new), `docs/features/creation.md` | `drawing_helpers.rs` (new) `each_helper_draws_its_geometry`: each helper's path operators equal the geometry the test computes. | — |

**Closes the row:** ED-46.1 holds each helper's path to the geometry computed in the test, which is the row's exit.

## ED-47 — Links to non-ASCII targets

**Links to non-ASCII targets**: IRIs percent-encoded to URIs on write

**Today.** refused (`features/creation.md:384`; `PageBuilder::link` returns `false`)

**Exit criterion.** fixtures with non-ASCII targets write the RFC 3987 mapping and read back

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-47.1 | build | IRIs mapped to URIs on write by RFC 3987 3.1 (UTF-8, then percent-encoding), in `PageBuilder::link` and every writer of URI actions. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf-cos/src/edit/annotations.rs`, `crates/tinker-pdf-cos/tests/iri_links.rs` (new), `docs/features/creation.md` | `iri_links.rs` (new) `non_ascii_targets_write_the_rfc_3987_mapping`: fixtures with non-ASCII hosts, paths and queries write the mapping and read back to it; failing first on today's refusal. | — |

**Closes the row:** ED-47.1 writes the RFC 3987 mapping for non-ASCII targets and reads it back, which is the row's exit.

## ED-48 — An SVG or another document's page placed as vector content

**An SVG or another document's page placed as vector content**: an SVG drawn as a form on a page or used as an image source in the builder

**Today.** SVG opens as its own document only

**Exit criterion.** the placed SVG renders as the opened SVG at that size

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-48.1 | build | An SVG placed as vector content: the page the SVG synthesises into drawn as a form on a builder or editor page at a given size. | `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf-cos/src/edit/stamp.rs`, `crates/tinker-pdf/tests/placed_vectors.rs` (new), `docs/features/creation.md` | `placed_vectors.rs` (new) `a_placed_svg_renders_as_the_opened_svg`: the placed SVG renders as the opened SVG at that size. | — |
| ED-48.2 | build | Another opened document's page as vector content, and an SVG as an image source in the builder. | `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/placed_vectors.rs` (new) | `placed_pages_render_as_their_source`: a placed page and an SVG image source render as their sources at that size. | ED-48.1 |

**Closes the row:** ED-48.1 renders a placed SVG as the opened SVG at that size, which is the row's exit; ED-48.2 adds other pages and the image source.

## ED-49 — Vector barcodes

**Vector barcodes**: Code 128 and GS1-128, Code 39 and 93, EAN-13 and 8, UPC-A and E, ITF-14, Codabar, MSI, postal codes, QR (all versions and error-correction levels, Kanji, structured append), Micro QR, rMQR, Data Matrix (ECC 200 and GS1), PDF417 and MicroPDF417, Aztec, MaxiCode, DataBar family, payment QR profiles; quiet zones and human-readable text

**Today.** absent — the owner's parity decision of 9 October 2026; "barcode" appears in two doc comments and no code (`rg -i 'barcode' crates`)

**Exit criterion.** each symbology held to its ISO/IEC specification's worked examples transcribed into tests; the decoder (ED-50) reads every generated symbol back; `tpdf barcode` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-49.1 | design | Create `docs/design/barcodes.md`: a new leaf crate, `tinker-pdf-barcode`, taking data and returning modules (bar widths or a bit matrix), PDF-free (ruling 8); the symbologies and each one's specification; which postal codes and payment QR profiles; quiet zones and human-readable text; and the decoder ED-50 as the read-back. | `docs/design/barcodes.md` (new), `docs/architecture.md` | The doc exists in the design-doc order and names ED-49.2 to ED-49.15's exit tests; `docs/architecture.md` is amended for the new leaf crate; reviewed. | — |
| ED-49.2 | build | The crate and its shared Reed-Solomon coding over the fields the 2D symbologies use, GF(256) with each symbology's polynomial and GF(929). | `crates/tinker-pdf-barcode/Cargo.toml` (new), `crates/tinker-pdf-barcode/src/lib.rs` (new), `crates/tinker-pdf-barcode/src/reed_solomon.rs` (new), `Cargo.toml`, `deny.toml` | Unit tests: each field's generator polynomials and check words equal the values the specifications publish. | ED-49.1 |
| ED-49.3 | build | Code 128 with GS1-128, Code 39 and Code 93. | `crates/tinker-pdf-barcode/src/linear.rs` (new) | Unit tests: each symbology's worked examples, transcribed from its specification, encode to the published bars. | ED-49.2, H-159 |
| ED-49.4 | build | EAN-13 and 8, UPC-A and E, and ITF-14. | `crates/tinker-pdf-barcode/src/linear.rs` (new) | Unit tests: the worked examples encode to the published bars, check digits included. | ED-49.3 |
| ED-49.5 | build | Codabar, MSI and the postal codes the design lists. | `crates/tinker-pdf-barcode/src/linear.rs` (new), `crates/tinker-pdf-barcode/src/postal.rs` (new) | Unit tests: the worked examples encode to the published bars and states. | ED-49.3 |
| ED-49.6 | build | QR: every version and error-correction level, the numeric, alphanumeric, byte and Kanji modes, masking and structured append. | `crates/tinker-pdf-barcode/src/qr.rs` (new) | Unit tests: the specification's worked example encodes to its published matrix, and every version and level builds a matrix of the published size and capacity. | ED-49.2, H-159 |
| ED-49.7 | build | Micro QR and rMQR. | `crates/tinker-pdf-barcode/src/qr.rs` (new) | Unit tests: the worked examples encode to their published matrices. | ED-49.6 |
| ED-49.8 | build | Data Matrix ECC 200 with the GS1 form. | `crates/tinker-pdf-barcode/src/datamatrix.rs` (new) | Unit tests: the worked example encodes to its published matrix; every symbol size has its published capacity. | ED-49.2, H-159 |
| ED-49.9 | build | PDF417 and MicroPDF417. | `crates/tinker-pdf-barcode/src/pdf417.rs` (new) | Unit tests: the worked examples encode to their published codewords and rows. | ED-49.2, H-159 |
| ED-49.10 | build | Aztec, compact and full-range. | `crates/tinker-pdf-barcode/src/aztec.rs` (new) | Unit tests: the worked example encodes to its published matrix. | ED-49.2, H-159 |
| ED-49.11 | build | MaxiCode. | `crates/tinker-pdf-barcode/src/maxicode.rs` (new) | Unit tests: the worked example encodes to its published hexagon grid. | ED-49.2, H-159 |
| ED-49.12 | build | The DataBar family. | `crates/tinker-pdf-barcode/src/databar.rs` (new) | Unit tests: each variant's worked examples encode to the published elements. | ED-49.2, H-159 |
| ED-49.13 | build | Payment QR profiles the design lists, as data formats over QR. | `crates/tinker-pdf-barcode/src/payment.rs` (new) | Unit tests: each profile's published example payload is produced field for field and encodes as QR. | ED-49.6 |
| ED-49.14 | build | Drawing: symbols written as vector paths on a builder page, with quiet zones and human-readable text, and `tpdf barcode`. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/barcodes.rs` (new), `docs/features/creation.md` | `barcodes.rs` (new) `symbols_draw_their_modules`: each drawn symbol's rectangles equal its modules at the given scale, quiet zone included; `tpdf barcode` runs each symbology to the facade's output. | ED-49.4, ED-49.5, ED-49.7, ED-49.8, ED-49.9, ED-49.10, ED-49.11, ED-49.12, ED-49.13 |
| ED-49.15 | test | Every generated symbol read back by ED-50's decoder, and a fingerprint of one page of symbols. | `crates/tinker-pdf/tests/barcodes.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | `every_generated_symbol_reads_back`: each symbology's generated symbols decode to their data; the fingerprint holds on every K-07 target. | ED-49.14, ED-50, K-07 |

**Closes the row:** ED-49.3 to ED-49.13 hold each symbology to its specification's worked examples, ED-49.15 reads every symbol back through ED-50, and ED-49.14 runs `tpdf barcode`, which is the row's exit.

## ED-50 — Barcode decoding

**Barcode decoding** from pages and images, at any rotation

**Today.** absent — the owner's parity decision of 9 October 2026; no symbology is decoded (ED-49's search)

**Exit criterion.** every symbol generated by ED-49 at every rotation, scale and quiet zone decodes to its input; published worked examples decode; `tpdf barcode --read` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** ED-49

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-50.1 | design | Add the decoder to `docs/design/barcodes.md`: bitmaps in from a rendered page or a decoded image, binarisation, linear symbols found by scanlines at every angle, 2D symbols by their finder patterns with rotation and perspective, sampling, error correction through ED-49.2's coding, and the published worked examples as the adjudicators besides ED-49's symbols. | `docs/design/barcodes.md` (new) | The section names ED-50.2 to ED-50.8's exit tests and is reviewed. | ED-49.14 |
| ED-50.2 | build | Linear symbols decoded at any rotation: Code 128 with GS1-128, Code 39 and 93, EAN and UPC, ITF-14, Codabar and MSI, found by scanlines across orientations of a binarised bitmap. | `crates/tinker-pdf-barcode/src/decode/linear.rs` (new), `crates/tinker-pdf-barcode/src/decode/mod.rs` (new), `crates/tinker-pdf-barcode/src/lib.rs` (new) | Unit tests: ED-49's generated linear symbols at every rotation in steps of 15 degrees, three scales and the minimum quiet zone decode to their input; the published worked examples decode. | ED-50.1 |
| ED-50.3 | build | Postal codes and the DataBar family decoded. | `crates/tinker-pdf-barcode/src/decode/linear.rs` (new) | Unit tests: ED-49's generated symbols at every rotation, scale and quiet zone decode to their input; the published worked examples decode. | ED-50.2 |
| ED-50.4 | build | QR, Micro QR and rMQR decoded: finder patterns located, the grid sampled through rotation and perspective, format and version read, Reed-Solomon corrected, every mode read. | `crates/tinker-pdf-barcode/src/decode/qr.rs` (new) | Unit tests: ED-49's generated symbols of every version and level at every rotation, scale and quiet zone decode to their input; the published worked example decodes. | ED-50.1 |
| ED-50.5 | build | Data Matrix decoded. | `crates/tinker-pdf-barcode/src/decode/datamatrix.rs` (new) | Unit tests: ED-49's generated symbols at every rotation, scale and quiet zone decode to their input; the worked example decodes. | ED-50.4 |
| ED-50.6 | build | PDF417 and MicroPDF417 decoded. | `crates/tinker-pdf-barcode/src/decode/pdf417.rs` (new) | Unit tests: ED-49's generated symbols at every rotation, scale and quiet zone decode to their input; the worked examples decode. | ED-50.2 |
| ED-50.7 | build | Aztec and MaxiCode decoded. | `crates/tinker-pdf-barcode/src/decode/aztec.rs` (new), `crates/tinker-pdf-barcode/src/decode/maxicode.rs` (new) | Unit tests: ED-49's generated symbols at every rotation, scale and quiet zone decode to their input; the worked examples decode. | ED-50.4 |
| ED-50.8 | build | The surface: symbols with their positions read from a page (rendered at a stated resolution) or an image, and `tpdf barcode --read`. | `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/barcodes.rs` (new), `docs/features/content-and-text.md` | A facade test reads every symbology from generated pages at several rotations; `tpdf barcode --read` runs over `testdata/` fixtures to the facade's output. | ED-50.3, ED-50.5, ED-50.6, ED-50.7 |

**Closes the row:** ED-50.2 to ED-50.7 decode every ED-49 symbol at every rotation, scale and quiet zone, and the published worked examples, and ED-50.8 runs `tpdf barcode --read`, which is the row's exit.

## ED-51 — Create a document from annotated content-stream text

**Create a document from annotated content-stream text** (media box, rotation, fonts by file or standard name, CJK fonts, images by comment)

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'from_content_text|annotated_content' crates tools` finds nothing

**Exit criterion.** the created document renders as the same content built through the builder; `tpdf create` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| ED-51.1 | build | A document from annotated content text: comment directives naming the media box, rotation, fonts by file or standard name, CJK fonts and images, followed by content operators, built into pages through the builder; the directive syntax stated in the feature doc. | `crates/tinker-pdf-cos/src/build_text.rs` (new), `crates/tinker-pdf-cos/src/lib.rs`, `crates/tinker-pdf-cos/tests/build_text.rs` (new), `docs/features/creation.md` | `build_text.rs` (new) `annotated_text_renders_as_its_builder_twin`: each fixture renders as the same content built through the builder; an unknown directive is a typed error naming its line. | — |
| ED-51.2 | build | `tpdf create` from an annotated text file. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | `tpdf create` runs over the annotated text fixtures to the facade's output. | ED-51.1 |

**Closes the row:** ED-51.1 renders the created document as its builder twin and ED-51.2 runs `tpdf create`, which is the row's exit.
