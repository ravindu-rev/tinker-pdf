# Capability tier — surface: CLI, bindings, packaging, API stability, documentation

[The roadmap](../ROADMAP.md) says how a step runs, what comes next, and which dependencies cross tiers. Each row below keeps its text as it was planned — Item, Today, Exit criterion, Doc, Depends on — and is cut into steps, each one workflow run.

Parity here means every capability reachable from a shell, from C and from
every binding, packaged where each ecosystem looks for it, with an API that
is checked for breaks and documented by examples that run.

## SU-01 — The CLI's verb coverage

**The CLI's verb coverage**: every capability row whose output a person runs from a shell carries its `tpdf` verb in its own exit criterion; the missing verbs, mapped: `sign`/`verify`/`timestamp`/`ltv` (SG-08), `redact` by area, search and pattern (ED-36a…f, SE-10), `annots` list/export/import/flatten/remove (AN-06, AN-08, AN-13), `forms` list/fill/export/flatten/reset/lock (AN-21, AN-22, AN-13, SG-15), `bookmarks` (ED-17), `labels` and `metadata` get and set (ED-17, SD-23), `attachments` list/add/remove/extract (ED-39), `layers` (ED-40), `convert` (FO-17, ED-42), `create` (ED-51), `optimise` (ED-24), `linearize`, `repair` and `clean` (ED-32, ED-37), `json` (SU-02), `show` (SU-05), `grep` across files (FT-21), `audit` (SU-07), `pages` and `info` inventories (SU-05), `trace` (FO-16), `compare` (ED-22, ED-23), `impose`, `overlay`, `watermark`, `header-footer`, `bates` (ED-08, ED-10, ED-14…16), `recolour` (ED-06a, ED-06b), `trim` (ED-07), `bake` (AN-13), `barcode` (ED-49, ED-50), `js` (FJ-15), `run` (SU-11), `ocr` (OC-11)

**Today.** eighteen subcommands, ten reading and eight writing (`tools/tpdf/src/main.rs:291-308`). The ten reading ones, `fonts` and `images` among them, and eight that write — `merge`, `split`, `rotate`, `encrypt`, `decrypt`, `attach`, `stamp`, `sanitise` — each a wrapper over the facade (ruling 11): `import_page`, `keep_pages`, `rotate_page`, the encryption on `WriteOptions`, `attach_file`, `import_page_as_form` with `stamp`, and `sanitise`. Three decisions the CLI first made on its own the facade now makes for every surface: a turn that is not a quarter (`rotate_page` refuses it), an empty owner password (the user's, as Algorithm 3 has it for R2 to R4), and `sanitise` with no flag (refused). A writer refuses a flag it would ignore, `--password` on an input that is not encrypted, and a page its list names twice. Every writer saves through `tinker_pdf::write::save`, takes `--font-policy subset|keep` and prints the pass's report; every writer rewrites, never appends; `split` garbage-collects and `merge` deduplicates; each asks `DocumentEditor::check_save` before it writes and maps the answer into its words (`an_encrypted_later_input_or_stamp_source_is_refused_by_name`). Held by `tools/tpdf/src/writing.rs`: each command run on `testdata/` fixtures and built documents, its output reopened through the facade and held to the strict validator

**Exit criterion.** `tpdf help` lists every verb and SU-10's convention suite runs over each; each verb's own test lands with its mapped row, whose exit criterion names the verb and runs it on `testdata/` fixtures, its output reopened through the facade and held to the strict validator

**Doc:** — · **Depends on:** each mapped row

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-01.1 | build | One verb table in `tpdf`: dispatch, `tpdf help` and each verb's synopsis built from it, each entry naming the row that owns the verb. | `tools/tpdf/src/main.rs`, `tools/tpdf/tests/verbs.rs` (new) | `verbs.rs` (new): `tpdf help` lists exactly the table's verbs; a table entry with no dispatch, or a dispatch with no entry, fails the test. | — |
| SU-01.2 | test | Every mapped verb present and held to the conventions, as each lands with the step of its row that runs it. | `tools/tpdf/tests/verbs.rs` (new) | `tpdf help` lists every verb the item maps; each verb's own test, landed with its row, runs it on `testdata/` fixtures with its output reopened and held to the strict validator; SU-10.6's suite passes over each. | SU-01.1, SU-10.6, SG-08.1, SG-08.2, SG-08.3, ED-36a.3, ED-36b.3, ED-36c.2, ED-36d.3, ED-36e.5, ED-36f.2, SE-10.2, AN-06.5, AN-08.5, AN-13.2, AN-21.3, AN-22.2, SG-15.3, ED-17.5, SD-23.6, ED-39.3, ED-40.3, FO-17.3, ED-42.2, ED-51.2, ED-24.6, ED-37.8, ED-32.3, SU-02.6, SU-05.1, SU-05.4, SU-05.5, FT-21.1, SU-07.2, FO-16.1, ED-22.4, ED-23.5, ED-08.6, ED-10.3, ED-16.2, ED-15.4, ED-14.2, ED-06a.4, ED-06b.8, ED-07.4, ED-49.14, ED-50.8, FJ-15.2, SU-11.4, OC-11 |

**Closes the row:** SU-01.1's table makes `tpdf help` list every verb, and SU-01.2 holds each mapped verb, landed with its row, to SU-10's convention suite, which is the row's exit.

**The tree at `5297133`, against this row's text:**

- Every mapped row already cut carries a step that runs its verb; only the rows cut here (SU-02, SU-05, SU-07, SU-11) and OC-11 lacked one, and SU-01.2 depends on each verb step by ID.

## SU-02 — A lossless JSON form of a document

**A lossless JSON form of a document**: every object, streams inline or as files, the trailer and cross-reference facts, filters by key and object, a schema description; JSON back to PDF, creating or updating with object numbers kept

**Today.** `tpdf objects` dumps an object (`--object N [--stream [--raw]]`)

**Exit criterion.** every committed fixture round-trips PDF → JSON → PDF to an equal object graph; the schema validates every export; `tpdf json` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-02.1 | build | PDF to JSON, losslessly: every object with its number and generation, each value kind distinguished (integers from reals, names from strings, string bytes kept), streams with their dictionaries and raw bytes, and the trailer and cross-reference facts: sections, kinds, offsets, object streams and free entries. | `crates/tinker-pdf/src/json_form.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/json_form.rs` (new) | `json_form.rs` (new): every committed fixture exports; each value kind has a unit test of its JSON form. | — |
| SU-02.2 | build | Streams inline or as files beside the JSON, raw or decoded, and filters by key and by object. | `crates/tinker-pdf/src/json_form.rs` (new), `crates/tinker-pdf/tests/json_form.rs` (new) | A filtered export holds exactly the selected objects; a stream written as a file reads back to the same bytes. | SU-02.1 |
| SU-02.3 | build | A schema description of the JSON form, and every export validated against it. | `crates/tinker-pdf/data/json-form.schema.json` (new), `crates/tinker-pdf/tests/json_form.rs` (new) | The schema validates every committed fixture's export in the test, and an export with one required field removed fails it. | SU-02.2 |
| SU-02.4 | build | JSON back to PDF as a new document, object numbers and generations kept. | `crates/tinker-pdf/src/json_form.rs` (new), `crates/tinker-pdf/tests/json_form.rs` (new) | Every committed fixture round-trips PDF to JSON to PDF to an equal object graph, and the output passes the strict validator. | SU-02.1 |
| SU-02.5 | build | JSON back to PDF as an update to an existing document: the changed objects written as an incremental update under their own numbers. | `crates/tinker-pdf/src/json_form.rs` (new), `crates/tinker-pdf/tests/json_form.rs` (new) | An export edited in the test and applied re-opens with the edit, the original revision byte-identical, and passes the strict validator. | SU-02.4 |
| SU-02.6 | build | `tpdf json`: export with the filters, import, and update. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | The command runs on `testdata/` fixtures in both directions to the facade's output, its writes reopened and held to the strict validator. | SU-02.3, SU-02.5 |

**Closes the row:** SU-02.4 round-trips every committed fixture to an equal object graph, SU-02.3's schema validates every export, and SU-02.6 runs `tpdf json` on `testdata/` fixtures, which is the row's exit.

## SU-03 — A text-editable normalised form

**A text-editable normalised form** — uncompressed, normalised, one object per line with original numbers as comments — and a fix-offsets pass so hand edits become valid again

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'fix_offsets|normali[sz]ed_form' crates tools` finds nothing

**Exit criterion.** a normalised fixture hand-edited in the test and fixed re-opens with the edit and validates

**Doc:** — · **Depends on:** ED-37

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-03.1 | build | The normalised form: every stream decompressed through ED-37.1, the syntax normalised through ED-37.5's pretty form, one object per line, each object's original number and generation kept as a comment. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf/tests/normalised_form.rs` (new) | `normalised_form.rs` (new): every committed fixture writes one object per line and re-opens to an equal object graph. | ED-37.1, ED-37.5 |
| SU-03.2 | build | A fix-offsets pass that makes hand edits valid again: cross-reference offsets, stream lengths and the trailer recomputed; `tpdf normalise` writes the form and `tpdf normalise --fix` repairs an edited one. | `crates/tinker-pdf-cos/src/write.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/normalised_form.rs` (new) | A normalised fixture hand-edited in the test and fixed re-opens with the edit and passes the strict validator; the command does both on `testdata/` fixtures. | SU-03.1 |

**Closes the row:** SU-03.2 re-opens a hand-edited normalised fixture with the edit, validated, which is the row's exit.

## SU-04 — A structural check with an exit-code contract

**A structural check with an exit-code contract**: categories (cross-reference, stream lengths, encryption, linearization and hint tables, object streams, page tree), documented exit codes, hint tables displayed

**Today.** `tpdf check --strict`

**Exit criterion.** each category's failure reached by a fixture and mapped to its documented exit code

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-04.1 | build | The structural check's categories over the strict validator's findings, cross-reference, stream lengths, encryption, linearization and hint tables, object streams and page tree, each with a documented exit code in `tpdf check`. | `crates/tinker-pdf-cos/src/validate.rs`, `tools/tpdf/src/main.rs`, `tools/tpdf/tests/check.rs` (new) | `check.rs` (new): each category's failure is reached by a fixture and exits with its documented code; a clean file exits zero. | — |
| SU-04.2 | build | Hint tables displayed: the page-offset and shared-object hint tables decoded and printed by `tpdf check --hints`. | `crates/tinker-pdf-cos/src/linearize.rs`, `tools/tpdf/src/main.rs`, `tools/tpdf/tests/check.rs` (new) | The printed tables of a linearized fixture equal the values the test decodes by hand; a damaged table falls in the linearization category. | SU-04.1 |

**Closes the row:** SU-04.1 reaches each category's failure with a fixture and maps it to its documented exit code, which is the row's exit.

## SU-05 — Inspection breadth

**Inspection breadth**: object paths and labels from the trailer, wildcard paths (`pages/*/Resources/Font/*/BaseFont`), cross-reference entries with object-stream indices, raw and decoded streams, encryption parameters, every script and URL, the structure tree with text, named destinations, every page box with paper-size names, effective image resolution per placement, font substitutions, a claimed-standards summary, per-page resource inventories

**Today.** `info`, `fonts`, `images`, `outline`, `objects`, `probe`

**Exit criterion.** each listing over the committed fixtures equals the facts walked independently in the test; `tpdf show`, `tpdf pages` and `tpdf info` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-05.1 | build | Object paths and labels from the trailer, wildcard paths such as `pages/*/Resources/Font/*/BaseFont`, and `tpdf show <path>`. | `crates/tinker-pdf/src/inspect.rs` (new), `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/inspection.rs` (new) | `inspection.rs` (new): each path over the committed fixtures resolves to the objects the test walks to; `tpdf show` prints them on `testdata/` fixtures. | — |
| SU-05.2 | build | Cross-reference entries with object-stream indices, raw and decoded streams, and encryption parameters, through `tpdf show`. | `crates/tinker-pdf/src/inspect.rs` (new), `crates/tinker-pdf/tests/inspection.rs` (new) | Each listing over the committed fixtures equals the facts walked independently in the test. | SU-05.1 |
| SU-05.3 | build | Every script and URL, the structure tree with its text, and named destinations. | `crates/tinker-pdf/src/inspect.rs` (new), `crates/tinker-pdf/tests/inspection.rs` (new) | Each listing over the committed fixtures equals the facts walked independently in the test. | SU-05.1 |
| SU-05.4 | build | Per page: every box with paper-size names, effective image resolution per placement, font substitutions and the resource inventory, through `tpdf pages`. | `crates/tinker-pdf/src/inspect.rs` (new), `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/inspection.rs` (new) | Each listing equals the facts walked independently in the test; `tpdf pages` runs on `testdata/` fixtures. | SU-05.1 |
| SU-05.5 | build | A claimed-standards summary and the document inventory in `tpdf info`. | `crates/tinker-pdf/src/inspect.rs` (new), `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/inspection.rs` (new) | The summary over each fixture equals the claims the validators read; `tpdf info` runs on `testdata/` fixtures. | SU-05.1 |

**Closes the row:** SU-05.1 to SU-05.5 make each listing equal the facts walked independently in the test and run `tpdf show`, `tpdf pages` and `tpdf info` on `testdata/` fixtures, which is the row's exit.

## SU-06 — Caller-tunable parser limits and a no-repair mode

**Caller-tunable parser limits and a no-repair mode**: the compiled caps adjustable within their ledger bounds per document, and a strict mode that fails on the first defect instead of repairing

**Today.** 65 compiled caps (`bounds_ledger.rs`); `--strict` validates after the fact

**Exit criterion.** each tunable cap held by `bounds_ledger.rs` to a ceiling; strict mode fails on each repair class's fixture

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-06.1 | build | Each compiled cap made tunable per document within its ledger ceiling, through the open options. | `crates/tinker-pdf-cos/src/limits.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/bounds_ledger.rs` | `bounds_ledger.rs` holds each tunable cap to a ceiling; a value past it is refused by a typed error; a lowered cap trips on a fixture that opens at the default. | — |
| SU-06.2 | test | Each repair class held to ED-32.1's refusing mode, and `--no-repair` on every reading verb. | `crates/tinker-pdf-cos/src/repair.rs`, `crates/tinker-pdf/tests/repair_on_demand.rs` (new), `tools/tpdf/src/main.rs` | Each repair class's fixture fails in the refusing mode with its defect named and opens repaired without it; the flag reaches the mode from every reading verb. | ED-32.1 |

**Closes the row:** SU-06.1 holds each tunable cap to a ceiling in `bounds_ledger.rs` and SU-06.2 fails each repair class's fixture in the strict mode, which is the row's exit.

**The tree at `5297133`, against this row's text:**

- ED-32.1 already builds the refuse-on-repair open option; SU-06.2 holds every repair class to it rather than building it twice.

## SU-07 — A space-usage audit

**A space-usage audit**: bytes by images, fonts, content, structure, forms, annotations, metadata, cross-reference data, thumbnails, attachments, unreferenced objects and old revisions, with the largest objects; operator frequencies

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'space_usage|space usage' crates tools` finds nothing

**Exit criterion.** the categories of each committed fixture sum to its length; `tpdf audit` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-07.1 | build | A space-usage audit: bytes by images, fonts, content, structure, forms, annotations, metadata, cross-reference data, thumbnails, attachments, unreferenced objects and old revisions, with the largest objects. | `crates/tinker-pdf/src/space_audit.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/space_audit.rs` (new) | `space_audit.rs` (new): the categories of each committed fixture sum to its length. | — |
| SU-07.2 | build | Operator frequencies over every content stream, and `tpdf audit`. | `crates/tinker-pdf/src/space_audit.rs` (new), `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/space_audit.rs` (new) | The frequencies over each fixture equal a count made in the test; `tpdf audit` runs on `testdata/` fixtures. | SU-07.1 |

**Closes the row:** SU-07.1's categories sum to each committed fixture's length and SU-07.2 runs `tpdf audit` on `testdata/` fixtures, which is the row's exit.

## SU-08 — Write-determinism controls

**Write-determinism controls**: object-stream policy (keep, disable, generate), a deterministic `/ID` from content, static IDs for tests, a newline before `endstream`, reproducible output stated as a guarantee

**Today.** `/ID` is already a SHA-256 of the serialised document, an inherited `/ID[0]` kept (`crates/tinker-pdf-cos/src/write.rs:948-990`); `WriteOptions::object_streams` generates or does not, with no keep policy; no static-identifier switch for tests

**Exit criterion.** the same input and options give byte-identical output on four targets; each switch held to a fixture

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-08.1 | build | The object-stream policy on `WriteOptions`: keep each object in the stream it came from, disable, or generate. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf-cos/src/objstm.rs`, `crates/tinker-pdf/tests/determinism.rs` | Each policy held to a fixture: keep preserves each object's stream membership, disable writes none, generate packs every eligible object. | — |
| SU-08.2 | build | A static-identifier switch for tests, the content-derived `/ID` stated as the default, and a switch for a newline before `endstream`. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf/tests/determinism.rs` | Each switch held to a fixture; with the static switch, writes of two different documents carry the documented constant. | — |
| SU-08.3 | ci | Reproducible output stated as a guarantee and checked: the same inputs and options written on four targets, the three operating systems and wasm, and the hashes compared. | `.github/workflows/ci.yml`, `crates/tinker-pdf/tests/determinism.rs`, `docs/features/determinism.md` | On a CI run on `develop` the four targets' hashes are byte-identical for every determinism fixture under every switch; the guarantee is written in `determinism.md`. | SU-08.1, SU-08.2, MS-11 |

**Closes the row:** SU-08.1 and SU-08.2 hold each switch to a fixture and SU-08.3 gives byte-identical output on four targets, which is the row's exit.

## SU-09 — Version management

**Version management**: a minimum or forced version, `/Extensions` written, 2.0-only features converted down for 1.4–1.7 readers

**Today.** `with_version` on the builder

**Exit criterion.** down-converted fixtures pass the object grammar (SD-20) at the target version

**Doc:** — · **Depends on:** SD-20

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-09.1 | build | A minimum or forced version on the writer: the header and the catalog's `/Version` set, a feature needing a higher version refused or raising it by the option's rule, and `/Extensions` written through SD-19.1. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/version_management.rs` (new) | `version_management.rs` (new): each option held to a fixture; a forced version below a feature in use is a typed refusal. | SD-19.1 |
| SU-09.2 | build | 2.0-only features converted down for 1.4 to 1.7 readers: UTF-8 strings re-encoded, namespaced structure mapped through the role map, and the page-level output intents and other 2.0 keys `pdf20-deltas.md` lists dropped or replaced. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf/tests/version_management.rs` (new), `docs/pdf20-deltas.md` | Down-converted fixtures pass the object grammar at the target version. | SU-09.1, SD-20.4 |

**Closes the row:** SU-09.2's down-converted fixtures pass SD-20's grammar at the target version, which is the row's exit.

## SU-10 — CLI conventions

**CLI conventions**: documented exit codes, `--json` on every reading verb with a schema, `-` for standard input and output, passwords from a file or the environment, one page-range grammar (ED-11), atomic in-place replacement, job files for reproducible batches, shell completion, `help <topic>`, progress on a terminal, quiet and verbose levels, directory batches with `--jobs`, a dry run for writers, `%d` in output names

**Today.** per-command flags

**Exit criterion.** a convention test suite over every verb

**Doc:** — · **Depends on:** ED-11

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-10.1 | build | Documented exit codes, one table for every verb, and `--json` on every reading verb with a schema per verb. | `tools/tpdf/src/main.rs`, `docs/features/cli.md` (new), `docs/README.md`, `tools/tpdf/tests/conventions.rs` (new) | `conventions.rs` (new): each documented code is reached by a fixture; each reading verb's `--json` output validates against its schema. | SU-01.1 |
| SU-10.2 | build | `-` for standard input and output, passwords from a file or the environment, atomic in-place replacement, and a dry run for writers. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs`, `tools/tpdf/tests/conventions.rs` (new) | Each convention is held over every verb it applies to; an interrupted in-place write leaves the original intact; a dry run writes nothing. | SU-01.1 |
| SU-10.3 | build | One page-range grammar, ED-11's, on every verb that takes pages. | `tools/tpdf/src/main.rs`, `tools/tpdf/tests/conventions.rs` (new) | Every page-taking verb accepts the grammar's forms and refuses a malformed range with the same error. | ED-11, SU-01.1 |
| SU-10.4 | build | Job files for reproducible batches, directory batches with `--jobs`, and `%d` in output names. | `tools/tpdf/src/main.rs`, `tools/tpdf/tests/conventions.rs` (new) | A job file run twice gives byte-identical outputs; a directory batch at any `--jobs` gives the outputs one job gives. | SU-01.1 |
| SU-10.5 | build | Shell completion, `help <topic>`, progress on a terminal, and quiet and verbose levels. | `tools/tpdf/src/main.rs`, `tools/tpdf/tests/conventions.rs` (new) | Completion scripts for the common shells list every verb and flag; progress appears on a terminal and never in piped output; each level's output is asserted. | SU-01.1 |
| SU-10.6 | test | The convention suite over every verb, driven by SU-01.1's table. | `tools/tpdf/tests/conventions.rs` (new) | Every verb in the table passes every convention that applies to it; a verb added to the table is covered with no new test. | SU-10.1, SU-10.2, SU-10.3, SU-10.4, SU-10.5 |

**Closes the row:** SU-10.6 runs the convention suite over every verb, which is the row's exit.

## SU-11 — `tpdf run`: scripts against the facade

**`tpdf run`: scripts against the facade** in the in-tree ECMAScript engine, with a REPL — the facade's object model exposed to scripts as a binding (ruling 11)

**Today.** absent — the owner's parity decision of 9 October 2026; `tpdf` has no `run` subcommand (`tools/tpdf/src/main.rs:291-308` lists eighteen)

**Exit criterion.** the parity script `bindings-parity` runs as a `tpdf run` script and prints the recorded hashes; `tpdf run` runs it on `testdata/` fixtures

**Doc:** `design/script-shell.md` (owed) · **Depends on:** FJ-03e, SU-12

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-11.1 | design | Create `docs/design/script-shell.md`: the facade's object model as a script binding under ruling 11, generated from the item list SU-12.1's coverage check reads; the in-tree engine's embedding; host capabilities (files under named roots, output, arguments, exit code); limits; the REPL; how `bindings-parity` runs as a script; milestones with exit tests. | `docs/design/script-shell.md` (new) | The doc exists in the design-doc order, names SU-11.2 to SU-11.6's exit tests, and is reviewed. | SU-12.1 |
| SU-11.2 | build | The script binding: facade types exposed to the engine as host objects for opening, reading, rendering, writing and validating, each a 1:1 projection. | `tools/tpdf/src/run/binding.rs` (new), `tools/tpdf/src/run/mod.rs` (new), `tools/tpdf/tests/run.rs` (new) | `run.rs` (new): each projected item gives through a script what the facade gives, compared in the test. | SU-11.1, FJ-03e |
| SU-11.3 | build | Host capabilities: files read and written under the roots the command line names, standard output and error, arguments and the exit code. | `tools/tpdf/src/run/host.rs` (new), `tools/tpdf/tests/run.rs` (new) | A script reaches files under a named root and is refused outside it; output, arguments and exit codes behave as documented. | SU-11.2 |
| SU-11.4 | build | `tpdf run script.js [args]`. | `tools/tpdf/src/main.rs`, `tools/tpdf/tests/run.rs` (new) | Scripts over `testdata/` fixtures print what the facade gives. | SU-11.3 |
| SU-11.5 | build | A REPL when `tpdf run` has no script: multi-line input, results printed, errors reported without leaving. | `tools/tpdf/src/run/repl.rs` (new), `tools/tpdf/tests/run.rs` (new) | A session fed from a pipe in the test gives each result in order and survives an error. | SU-11.4 |
| SU-11.6 | test | The parity scripts as `tpdf run` scripts, a line of their own in `bindings-parity`. | `xtask/src/parity.rs`, `tools/tpdf/tests/run.rs` (new) | `cargo xtask bindings-parity` runs each script under `tpdf run` on `testdata/` fixtures and it prints the recorded hashes. | SU-11.4, SU-12.24 |

**Closes the row:** SU-11.6 runs the parity scripts under `tpdf run` printing the recorded hashes on `testdata/` fixtures, which is the row's exit.

## SU-12 — Every facade item on every surface

**Every facade item on every surface** (widened from the bindings row): the owed builder, editor, graphics-writing, tagged-writing, signature, public-key, timestamp, standalone-opener, `to_svg`, `display_list` and recoding projections, and the untracked ones — redaction, PDF/A, PDF/UA and PDF/X validation, search, structured-text writers, reading order and tables, font listing, `render_form`/`render_annotation`, layers, the subset report, `check_save` and `check_decrypt` — plus each row the roadmap adds; a generated coverage check

**Today.** **The C ABI exports 221 functions today, and the Go and Java bindings call every one of them** ([features/bindings.md](../features/bindings.md)). **Owed projections**: the rest of `PageBuilder` (`glyphs`, `text_pieces`, `shading`) and of `DocumentBuilder` (`add_cid_font`, `glyph_run`, `add_shading`) — the graphics states, form XObjects, tiling patterns and their page setters, `add_named_font` with `encoded_text`, `set_bleed_box`, `with_version` and `clear_image_resources` crossed in October 2026 — and the builder methods: `add_shading_pattern`, `add_icc_color_space` with `set_fill_icc` / `set_stroke_icc` and `ImageColorSpace::Icc`, the October 2026 CIE writer that is its twin (`CieSpace` with `add_cie_color_space`, `set_fill_cie` / `set_stroke_cie`, and `ImageColorSpace::Cie` and `ImageColorSpace::Lab`), `associate_file` on both builders with the October 2026 holders (`add_form_with_files`, `associate_file_with_structure`, `associate_file_with_link`, `with_associated_files`) and their readers (`associated_files_of`, `structure_associated_files`, `annotation_associated_files`, `marked_content_associated_files`), `PageBuilder::output_intent` and `link_for`, `finish_reporting` with the `EmbeddedWhole` list it returns, `dangling_destinations`, and the archival builder (`archival`, `profile`, `refusals`, `finish_archival`); `DocumentEditor`'s `import_page`, `keep_pages`, `flatten_annotations`, `add_annotation`, `reset_form`, `set_field_values`, `set_calculated_values`, `check_save` and `check_decrypt` with `SaveRefusal` (the save doors' behaviour is H-10's), and of the document operations `set_viewer_preferences` (the rest — page labels, attachments, the outline, the typed `/Info` setters, XMP, the page boundaries and `sanitise` with its report — crossed in October 2026); `WriteOptions::deduplicate_streams` (the C ABI builds it at its default); the September 2026 graphics-writing surface — `Target::Named` and both `add_named_destination`s, `DocumentBuilder::add_layer` with `PageBuilder::optional` and `DocumentEditor::set_layer_visible`, `add_separation_color_space` and `add_device_n_color_space` with `Function::Calculator` and `DeviceNAttributes`, `set_fill_tint` / `set_stroke_tint` and `ImageColorSpace::Tint`, `DocumentEditor`'s `stamp`, `add_resource`, `add_form` and `import_page_as_form`, and `Page::images` with `PageImage`; of the forms surface, `SigningTarget::NewVisibleField` with `SignatureAppearance` (a signing target, so it waits on signing crossing at all, SG-09) and the annotation payloads with `Page::annotation_list` (the rest — `DocumentEditor::add_field` and `NewField`, and `form_data`'s FDF and XFDF reading, writing and applying — crossed in October 2026); of tagged writing, a tag's `table`, `namespace` and `associated_file`, `continue_at`, `map_role_in`, `add_namespace` and `duplicate_element_ids` (`PageBuilder::tagged` as the `open_tag`/`close_tag` pair, with `set_language` and `map_role`, crossed in October 2026); a signature's `/Contents`, `/M`, `/ContactInfo`, `/Filter` and warnings through the C ABI (Python and JavaScript carry them), and `modifications` on every surface; `authenticate_with_recipient`; the October 2026 signature and public-key surface — `DocumentEditor::save_timestamped` with `Timestamper` and `TimestampRequest`, `add_validation_data` with `ValidationData`, `Document::security_store` with `SecurityStore` and its warnings, `PublicKeyEncryption::seal` with `EntropySource` and `DocumentEditor::save_sealed`, `Verdict::timestamps` with `TimestampVerdict`, and `Signature::validation_key`; and the wave 4–6 openers and outputs — the standalone-document openers (`Document::open_with` and `sniff`), `FromHtml`, `Document::open_markdown` and FB2, `Page::to_svg` and `DisplayList::to_svg` with `SvgOptions`, `Page::display_list` (retained pages) and image recompression (`recode`). The read half of the owed list has no section in [design/bindings-write.md](../design/bindings-write.md); the ten facade areas named last in the item reach no binding and no row tracked them before this one (a grep of the C header against the facade, 9 October 2026)

**Exit criterion.** each through the C ABI and every wrapper over it — .NET, Go, Ruby, Java — and Python and JavaScript, with a parity-script line; `cargo xtask bindings-coverage` lists every public facade item against the C ABI and each binding and fails on a gap not named in `features/bindings.md`'s refusal table; `cargo xtask bindings-parity` green

**Doc:** [design/bindings-write.md](../design/bindings-write.md) (read half added) · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-12.1 | ci | `cargo xtask bindings-coverage`: every public facade item listed against the C ABI and each binding, generated from the facade's public items and each surface's exports, failing on a gap that is neither in `features/bindings.md`'s refusal table nor in an owed list naming the step of this row that closes it. | `xtask/src/coverage.rs` (new), `xtask/src/main.rs`, `docs/features/bindings.md`, `.github/workflows/ci.yml` | The check runs in CI on `develop`; today's gaps are each owed to a step below by name; a facade item added with no projection and no owed or refused entry fails it. | MS-11 |
| SU-12.2 | design | Add the read half to `docs/design/bindings-write.md`: how read-only items project (iterators, borrowed data, verdicts and their warnings), and the recipient callback public-key opening needs beside SG-09.4's signing, timestamper and entropy callbacks. | `docs/design/bindings-write.md` | The section exists, names the steps below that follow it, and is reviewed. | — |
| SU-12.3 | build | Builder text and fonts on every surface: `PageBuilder::glyphs` and `text_pieces`, `DocumentBuilder::add_cid_font` and `glyph_run`. | `crates/tinker-pdf-ffi/src/graphics.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1 |
| SU-12.4 | build | Builder shading and colour on every surface: `PageBuilder::shading`, `add_shading`, `add_shading_pattern`, `add_icc_color_space` with `set_fill_icc`, `set_stroke_icc` and `ImageColorSpace::Icc`, and the CIE writer: `CieSpace`, `add_cie_color_space`, `set_fill_cie`, `set_stroke_cie`, `ImageColorSpace::Cie` and `ImageColorSpace::Lab`. | `crates/tinker-pdf-ffi/src/graphics.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1 |
| SU-12.5 | build | Associated files, page output intents and links on every surface: `associate_file` on both builders with `add_form_with_files`, `associate_file_with_structure`, `associate_file_with_link` and `with_associated_files`; their readers `associated_files_of`, `structure_associated_files`, `annotation_associated_files` and `marked_content_associated_files`; `PageBuilder::output_intent` and `link_for`. | `crates/tinker-pdf-ffi/src/graphics.rs`, `crates/tinker-pdf-ffi/src/read.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1, SU-12.2 |
| SU-12.6 | build | Builder finishing and the archival builder on every surface: `finish_reporting` with its `EmbeddedWhole` list, `dangling_destinations`, and `archival`, `profile`, `refusals` and `finish_archival`. | `crates/tinker-pdf-ffi/src/graphics.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1 |
| SU-12.7 | build | Editor pages and annotations on every surface: `import_page`, `keep_pages`, `flatten_annotations`, `add_annotation`, and the annotation payloads with `Page::annotation_list`. | `crates/tinker-pdf-ffi/src/docops.rs`, `crates/tinker-pdf-ffi/src/read.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1, SU-12.2 |
| SU-12.8 | build | Editor forms on every surface: `reset_form`, `set_field_values` and `set_calculated_values`. | `crates/tinker-pdf-ffi/src/forms.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1 |
| SU-12.9 | build | Save checks and options on every surface: `check_save` and `check_decrypt` with `SaveRefusal`, the save doors behaving as H-10 decides, `set_viewer_preferences`, and `WriteOptions::deduplicate_streams`. | `crates/tinker-pdf-ffi/src/docops.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1 |
| SU-12.10 | build | Destinations and layers on every surface: `Target::Named` and both `add_named_destination`s, `DocumentBuilder::add_layer` with `PageBuilder::optional`, and `DocumentEditor::set_layer_visible`. | `crates/tinker-pdf-ffi/src/graphics.rs`, `crates/tinker-pdf-ffi/src/docops.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1 |
| SU-12.11 | build | Spot colour on every surface: `add_separation_color_space` and `add_device_n_color_space` with `Function::Calculator` and `DeviceNAttributes`, `set_fill_tint`, `set_stroke_tint` and `ImageColorSpace::Tint`. | `crates/tinker-pdf-ffi/src/graphics.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1 |
| SU-12.12 | build | Stamping and form XObjects on every surface: `DocumentEditor`'s `stamp`, `add_resource`, `add_form` and `import_page_as_form`, and `Page::images` with `PageImage`. | `crates/tinker-pdf-ffi/src/docops.rs`, `crates/tinker-pdf-ffi/src/read.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1, SU-12.2 |
| SU-12.13 | build | Tagged writing on every surface: a tag's `table`, `namespace` and `associated_file`, `continue_at`, `map_role_in`, `add_namespace` and `duplicate_element_ids`. | `crates/tinker-pdf-ffi/src/tagging.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1 |
| SU-12.14 | build | Signature reading on every surface: a signature's `/Contents`, `/M`, `/ContactInfo`, `/Filter` and warnings through the C ABI, which Python and JavaScript already carry, and `modifications` everywhere. | `crates/tinker-pdf-ffi/src/read.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; the signatures parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1, SU-12.2 |
| SU-12.15 | build | Timestamps and validation data on every surface: `save_timestamped` with `Timestamper` and `TimestampRequest` over SG-09.4's callback, `add_validation_data` with `ValidationData`, `Document::security_store` with `SecurityStore` and its warnings, `Verdict::timestamps` with `TimestampVerdict`, and `Signature::validation_key`. | `crates/tinker-pdf-ffi/src/docops.rs`, `crates/tinker-pdf-ffi/src/read.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line with a deterministic timestamper prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1, SU-12.2, SG-09.4 |
| SU-12.16 | build | Public-key encryption on every surface: `authenticate_with_recipient` over SU-12.2's recipient callback, and `PublicKeyEncryption::seal` with `EntropySource` over SG-09.4's and `DocumentEditor::save_sealed`. | `crates/tinker-pdf-ffi/src/docops.rs`, `crates/tinker-pdf-ffi/src/lib.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line with fixed entropy prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1, SU-12.2, SG-09.4 |
| SU-12.17 | build | A visible signature on every surface: `SigningTarget::NewVisibleField` with `SignatureAppearance`, over SG-09's signing routes. | `crates/tinker-pdf-ffi/src/forms.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; the deterministically signed parity line with a visible field prints the recorded hash on every surface; the owed entries leave the coverage check. | SU-12.1, SG-09 |
| SU-12.18 | build | The standalone openers on every surface: `Document::open_with` and `sniff`, `FromHtml`, `Document::open_markdown` and FB2. | `crates/tinker-pdf-ffi/src/lib.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1 |
| SU-12.19 | build | Outputs on every surface: `Page::to_svg` and `DisplayList::to_svg` with `SvgOptions`, `Page::display_list`, and image recompression through `recode`. | `crates/tinker-pdf-ffi/src/read.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1, SU-12.2 |
| SU-12.20 | build | Validation on every surface: PDF/A, PDF/UA and PDF/X verdicts with their findings and abstentions. | `crates/tinker-pdf-ffi/src/read.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each verdict answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new read parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1, SU-12.2 |
| SU-12.21 | build | Redaction and search on every surface. | `crates/tinker-pdf-ffi/src/docops.rs`, `crates/tinker-pdf-ffi/src/read.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1, SU-12.2 |
| SU-12.22 | build | Text structure on every surface: the structured-text writers, reading order and tables, and font listing. | `crates/tinker-pdf-ffi/src/read.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new read parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1, SU-12.2 |
| SU-12.23 | build | Rendering pieces and reports on every surface: `render_form` and `render_annotation`, the layer listing, and the subset report. | `crates/tinker-pdf-ffi/src/read.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each item answers through the C ABI, .NET, Go, Ruby, Java, Python and JavaScript; a new parity line prints the recorded hash on every surface; the area's owed entries leave the coverage check. | SU-12.1, SU-12.2 |
| SU-12.24 | ci | Nothing owed: the coverage check and `bindings-parity` green across every surface, the .NET column read from its CI leg, which is H-104's alternative. | `xtask/src/coverage.rs` (new), `xtask/src/parity.rs`, `.github/workflows/ci.yml`, `docs/features/bindings.md` | On a CI run on `develop`, `bindings-coverage` lists every facade item as projected or refused by name with no owed entry, and `bindings-parity --require-all` prints every recorded hash on every surface. | SU-12.3, SU-12.4, SU-12.5, SU-12.6, SU-12.7, SU-12.8, SU-12.9, SU-12.10, SU-12.11, SU-12.12, SU-12.13, SU-12.14, SU-12.15, SU-12.16, SU-12.17, SU-12.18, SU-12.19, SU-12.20, SU-12.21, SU-12.22, SU-12.23 |

**Closes the row:** SU-12.3 to SU-12.23 carry each named item through the C ABI and every binding with a parity line, and SU-12.24 holds `bindings-coverage` with nothing owed and `bindings-parity` green, which is the row's exit; the .NET column is read from its CI leg, H-104's alternative, so verifying the binding by hand where an SDK exists (H-104) is outside this exit.

**The tree at `5297133`, against this row's text:**

- SG-09.4 carries the signing, timestamper and entropy callbacks across the C ABI; SU-12.15 and SU-12.16 build on it, and SU-12.2 adds only the read half and the recipient callback.

## SU-13 — A Swift binding that has been built; Ruby's streaming open

**A Swift binding that has been built; Ruby's streaming open**

**Today.** **Go, Ruby and Java landed, October 2026**, in that order, each a thin wrapper over the C header with a smoke program and a parity program, wired into `cargo xtask bindings-parity` and `ci.yml`'s `bindings-parity` job, and each printing all thirteen recorded hashes on linux/x86_64; Go (cgo) and Java (FFM, JDK 21 with `--enable-preview`, flagless from 22 — the 22 half unverified, H-105) call all 221 C functions, Ruby (Fiddle) 220, refusing the streaming open by name because Fiddle passes no struct by value. **Swift is source only**: a SwiftPM package over the committed header covering 49 functions, never compiled, because no Swift toolchain was available ([features/bindings.md](../features/bindings.md))

**Exit criterion.** Swift built on a macOS CI leg, its smoke run and a parity program printing the recorded hashes, wired into `bindings-parity` and CI, and widened to the header; Ruby's streaming open through a by-pointer vtable on the C ABI with callbacks that are safe off the Ruby thread, or kept refused by name

**Doc:** — · **Depends on:** H-103

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-13.1 | ci | A macOS CI leg with a Swift toolchain, building the SwiftPM package over the committed header and running its smoke program. | `.github/workflows/ci.yml`, `bindings/swift` | On a CI run on `develop` the package builds and its smoke program runs. | H-103, MS-11 |
| SU-13.2 | build | Swift widened to the whole header, with a parity program printing the recorded hashes, wired into `bindings-parity`. | `bindings/swift`, `xtask/src/parity.rs`, `.github/workflows/ci.yml` | On the macOS leg Swift calls every function the header exports and prints every recorded hash. | SU-13.1 |
| SU-13.3 | build | Ruby's streaming open through a by-pointer vtable on the C ABI with callbacks safe off the Ruby thread, or kept refused by name. | `crates/tinker-pdf-ffi/src/lib.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/ruby`, `docs/features/bindings.md` | Ruby opens a fixture from a streaming source to the recorded read hash and calls all 221 functions, or the refusal stays in the refusal table with its reason. | — |

**Closes the row:** SU-13.1 and SU-13.2 build Swift on a macOS leg, widened to the header, printing the recorded hashes, and SU-13.3 settles Ruby's streaming open, which is the row's exit.

## SU-14 — Binding ergonomics

**Binding ergonomics**: typed exceptions, Promise-returning APIs and a worker helper in JavaScript, cancellation and progress everywhere, stream adapters (file-like objects, Node streams, .NET `Stream`, Java `InputStream`) over `ByteSource`, and a CommonJS entry that re-exports the one ESM build

**Today.** ESM only; a CommonJS build refused because "two builds of the engine can diverge" (`features/bindings.md:1051`)

**Exit criterion.** each binding's test suite exercises each shape; the CommonJS entry loads the same wasm bytes as the ESM one (hash checked)

**Doc:** — · **Depends on:** SU-12

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-14.1 | build | Typed exceptions in every binding, one class per status the C ABI and the facade return. | `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js` | Each binding's tests raise every status as its typed exception. | SU-12 |
| SU-14.2 | build | JavaScript: Promise-returning APIs and a worker helper, and a CommonJS entry that re-exports the one ESM build. | `bindings/js`, `docs/features/bindings.md` | The CommonJS entry loads the same wasm bytes as the ESM one, hash checked; each API shape is exercised in `bindings/js/tests`. | SU-12 |
| SU-14.3 | build | Cancellation and progress on every binding, over the facade's budgets and SU-21's callbacks. | `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js` | Each binding cancels a long render part-way and receives its progress events in order. | SU-12, SU-21 |
| SU-14.4 | build | Stream adapters over `ByteSource`: file-like objects in Python, Node streams, .NET `Stream`, Java `InputStream` and Go's readers. | `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/python`, `bindings/js` | Each binding opens a fixture from its stream type to the same read hash as from bytes. | SU-12, P-05.7, P-05.8 |

**Closes the row:** SU-14.1 to SU-14.4 exercise each shape in each binding's tests and SU-14.2 checks the CommonJS entry's wasm hash, which is the row's exit.

## SU-15 — More platforms

**More platforms**: a C++ header-only wrapper over the C ABI; Android through JNI and an AAR per ABI; an iOS XCFramework

**Today.** C ABI, .NET, Go, Ruby, Java (FFM), Python, JavaScript

**Exit criterion.** each prints the recorded hashes in `bindings-parity` on its platform's CI leg

**Doc:** — · **Depends on:** SU-13

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-15.1 | build | A C++ header-only wrapper over the C ABI, with handles that release themselves and status as exceptions, and its parity program. | `bindings/cpp` (new), `xtask/src/parity.rs`, `.github/workflows/ci.yml` | On its CI leg the wrapper prints the recorded hashes in `bindings-parity`. | — |
| SU-15.2 | build | Android through JNI over the C ABI and an AAR per ABI, with its parity program on an emulator leg. | `bindings/android` (new), `xtask/src/parity.rs`, `.github/workflows/ci.yml` | On its CI leg the AAR's parity program prints the recorded hashes. | — |
| SU-15.3 | build | An iOS XCFramework with the Swift package over it, and its parity program on a simulator leg. | `bindings/swift`, `xtask/src/parity.rs`, `.github/workflows/ci.yml` | On its CI leg the parity program prints the recorded hashes. | SU-13.2 |

**Closes the row:** SU-15.1 to SU-15.3 each print the recorded hashes in `bindings-parity` on their platform's CI leg, which is the row's exit; publishing the AAR and the Swift package, signed and notarised, is H-115's release engineering, outside this exit.

## SU-16 — A high-level convenience layer per binding

**A high-level convenience layer per binding**: one-call open, render, extract, fill, sign, convert and save helpers mirroring facade presets

**Today.** the bindings project the facade 1:1

**Exit criterion.** each helper's output equals the long-hand facade sequence's (byte hash)

**Doc:** — · **Depends on:** SU-12

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-16.1 | build | Presets on the facade first, since a binding adds no logic of its own (ruling 11): one-call open, render, extract, fill, sign, convert and save, each over the long-hand calls. | `crates/tinker-pdf/src/presets.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/presets.rs` (new) | `presets.rs` (new): each helper's output equals the long-hand facade sequence's, by byte hash. | — |
| SU-16.2 | build | The presets on every binding, each a 1:1 projection with a parity line. | `crates/tinker-pdf-ffi/src/lib.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `crates/tinker-pdf/examples/write_parity.rs` | Each helper's output on every surface equals the long-hand sequence's hash, printed by `bindings-parity`. | SU-16.1, SU-12, SG-09 |

**Closes the row:** SU-16.1 and SU-16.2 make each helper's output equal the long-hand facade sequence's hash on every surface, which is the row's exit.

**The tree at `5297133`, against this row's text:**

- The facade has no presets today, so a binding helper would add logic ruling 11 forbids; SU-16.1 puts them on the facade first.

## SU-17 — Packaging, the machine half

**Packaging, the machine half**: crates.io and docs.rs (a publishing policy for leaf crates), wheels for manylinux and musllinux (x86-64 and aarch64), macOS universal2, Windows x64 and arm64 with type stubs, npm with a SIMD variant and the worker helper, NuGet with every runtime identifier, Maven Central classifiers and an FFM variant, a Go module with prebuilt static libraries, native gems, a SwiftPM XCFramework, recipes for conda, Homebrew, the major distributions, a container image and winget, pkg-config and CMake files, vcpkg and Conan ports, a versioned shared object

**Today.** nothing published: `pip install`, `npm install` and `dotnet add package` do not work, by decision, until the facade freezes at 0.1.0 ([features/bindings.md](../features/bindings.md)); `release.yml` builds crates, wheels, npm, natives and NuGet, has run once and published nothing

**Exit criterion.** a release dry run (`workflow_dispatch` without `publish`) builds and installs every artefact on its platform and renders a page

**Doc:** — · **Depends on:** H-113, H-115

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-17.1 | ci | crates.io and docs.rs: a publishing policy for leaf crates written, and every crate packaged in publishing order with docs.rs metadata. | `Cargo.toml`, `.github/workflows/ci.yml`, `CONTRIBUTING.md` | `cargo package` succeeds for every crate in publishing order in a CI job on `develop`, and the policy names which leaf crates publish. | H-113, MS-11 |
| SU-17.2 | ci | Wheels for manylinux and musllinux on x86-64 and aarch64, macOS universal2, and Windows x64 and arm64, with type stubs. | `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `bindings/python/pyproject.toml` | A CI job on `develop` builds each wheel and installs and smoke-runs it on its platform. | H-113, MS-11 |
| SU-17.3 | ci | npm with a SIMD variant and the worker helper. | `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `bindings/js` | A CI job packs both wasm variants with the worker helper and runs the smoke test from the packed tarball. | SU-14.2, H-113 |
| SU-17.4 | ci | NuGet with every runtime identifier. | `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `bindings/dotnet` | A CI job packs every runtime identifier and a smoke project restores the package from a local feed on each platform. | H-113 |
| SU-17.5 | ci | Maven Central classifiers and an FFM variant, a Go module with prebuilt static libraries, and native gems. | `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `bindings/java`, `bindings/go`, `bindings/ruby` | CI jobs build each artefact and a smoke program consumes it on each platform. | H-113, H-115 |
| SU-17.6 | ci | A SwiftPM XCFramework, recipes for conda, Homebrew and the major distributions, a container image, and winget. | `packaging` (new), `.github/workflows/ci.yml`, `bindings/swift` | Each recipe builds in CI from the source archive and its package runs the smoke test; the container image runs `tpdf` on a fixture. | SU-15.3, H-115 |
| SU-17.7 | ci | pkg-config and CMake files, and vcpkg and Conan ports, over SU-18.2's versioned shared object. | `packaging` (new), `crates/tinker-pdf-ffi/Cargo.toml`, `.github/workflows/ci.yml` | A C program finds the library through pkg-config, through CMake and through each port in CI. | SU-18.2 |
| SU-17.8 | watch | The release dry run: `release.yml` dispatched by the owner without `publish`, building and installing every artefact on its platform and rendering a page. | `.github/workflows/release.yml`, `docs/features/bindings.md` | On the owner's dispatch, read to the end, every artefact builds and installs on its platform and the page renders. | SU-17.1, SU-17.2, SU-17.3, SU-17.4, SU-17.5, SU-17.6, SU-17.7, H-108 |

**Closes the row:** SU-17.8's release dry run builds and installs every artefact SU-17.1 to SU-17.7 package and renders a page, which is the row's exit.

**The tree at `5297133`, against this row's text:**

- Only the owner starts the Release workflow, so each packaging step proves itself in a `ci.yml` job and the dry run itself waits on the owner (H-108).

## SU-18 — API stability, checked

**API stability, checked**: a semver diff of the facade in CI, versioned C structures with size fields and a versioned shared object, a deprecation window, an MSRV policy, a changelog, experimental markers

**Today.** "the facade freezes at 0.1.0"; nothing checks a break

**Exit criterion.** the CI diff fails on an unannounced break; every C structure carries its size; the MSRV built in CI

**Doc:** — · **Depends on:** H-114, H-16 (tool allowlist)

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-18.1 | doc | A deprecation window, an MSRV policy, a changelog, and experimental markers. | `CONTRIBUTING.md`, `CHANGELOG.md` (new), `crates/tinker-pdf/src/lib.rs` | Each policy is written; experimental items carry the marker and are left out of the diff. | — |
| SU-18.2 | build | Versioned C structures, each carrying its size as its first field and checked on entry, and a versioned shared object. | `crates/tinker-pdf-ffi/src/lib.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `crates/tinker-pdf-ffi/cbindgen.toml`, `crates/tinker-pdf-ffi/tests/layout.rs` | Every `#[repr(C)]` structure carries its size and one passed short is refused by status; the built library carries its versioned name. | — |
| SU-18.3 | ci | A semver diff of the facade against the frozen release, in CI. | `.github/workflows/ci.yml`, `xtask/src/main.rs` | The CI diff fails on a break not announced in the changelog, shown by an injected break. | H-16, H-114, SU-18.1 |
| SU-18.4 | ci | The MSRV built in CI. | `.github/workflows/ci.yml`, `Cargo.toml` | A CI leg builds the workspace at the stated MSRV on `develop`. | SU-18.1, MS-11 |

**Closes the row:** SU-18.3 fails CI on an unannounced break, SU-18.2 sizes every C structure, and SU-18.4 builds the MSRV in CI, which is the row's exit.

## SU-19 — The documentation set

**The documentation set**: rustdoc examples everywhere; a guide and a task cookbook ("fill a form", "sign with a key file", "make PDF/A"); per-binding references; runnable examples per binding in CI; manual pages; a catalogue of every typed warning with its meaning and remedy; a browser playground on the wasm build

**Today.** feature docs and design docs; seven examples run in CI

**Exit criterion.** every example compiles and runs in CI; the warning catalogue is generated from the source and fails on an undocumented variant

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-19.1 | build | Rustdoc examples on every public facade item, run as doctests, and a check listing the items without one. | `crates/tinker-pdf/src/lib.rs`, `xtask/src/doc_examples.rs` (new), `xtask/src/main.rs` | The check fails on a public item without an example; doctests pass in both builds in CI. | — |
| SU-19.2 | doc | A guide and a task cookbook, fill a form, sign with a key file, make PDF/A and the rest, each recipe a compiled example. | `docs/guide` (new), `crates/tinker-pdf/examples`, `docs/README.md` | Every recipe's code compiles and runs in CI. | — |
| SU-19.3 | doc | Per-binding references and runnable examples per binding. | `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js`, `.github/workflows/ci.yml` | Every binding example runs in CI. | — |
| SU-19.4 | doc | Manual pages for `tpdf`, generated from SU-01.1's verb table. | `tools/tpdf/src/main.rs`, `tools/tpdf/man` (new) | A page per verb is generated and a verb without one fails the generator. | SU-01.1 |
| SU-19.5 | build | A catalogue of every typed warning with its meaning and remedy, generated from the source. | `xtask/src/warnings.rs` (new), `docs/warnings.md` (new), `docs/README.md` | The generator fails on a warning variant without a meaning and remedy, and CI regenerates the catalogue and fails on a difference. | — |
| SU-19.6 | build | A browser playground on the wasm build: examples edited and run against the JavaScript binding. | `bindings/js/demo`, `.github/workflows/ci.yml` | A headless-browser test runs each playground example and gets the binding's output. | — |

**Closes the row:** SU-19.1 to SU-19.4 run every example in CI and SU-19.5's catalogue fails on an undocumented variant, which is the row's exit.

## SU-20 — Supply-chain evidence

**Supply-chain evidence**: an SBOM per artefact, build provenance, signed releases, attestations on publishing, a reproducible-build check

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'sbom|cyclonedx|attest' .github` finds nothing

**Exit criterion.** the release dry run emits each and a second build reproduces the artefacts' hashes

**Doc:** — · **Depends on:** SU-17

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-20.1 | ci | An SBOM per artefact and build provenance, emitted by the release workflow. | `.github/workflows/release.yml` | The workflow's jobs write an SBOM and a provenance statement for every artefact, checked by a CI job that runs the same steps without publishing. | SU-17.1, SU-17.2, SU-17.3, SU-17.4, SU-17.5, SU-17.6, SU-17.7 |
| SU-20.2 | ci | Signed releases and attestations on publishing. | `.github/workflows/release.yml` | The publishing jobs sign each artefact and attach an attestation, verified by the job that runs them without publishing. | SU-20.1, H-115 |
| SU-20.3 | ci | A reproducible-build check: each artefact built a second time and compared by hash. | `.github/workflows/release.yml`, `.github/workflows/ci.yml` | A second build reproduces every artefact's hash, or each difference is fixed by class. | SU-17.1, SU-17.2, SU-17.3, SU-17.4, SU-17.5, SU-17.6, SU-17.7 |
| SU-20.4 | watch | The release dry run read for the three. | `.github/workflows/release.yml`, `docs/features/bindings.md` | On the owner's dispatch without `publish`, read to the end, the run emits each and the second build reproduces the artefacts' hashes. | SU-20.1, SU-20.2, SU-20.3, H-108 |

**Closes the row:** SU-20.4's dry run emits SU-20.1 and SU-20.2's evidence and SU-20.3's second build reproduces the hashes, which is the row's exit.

## SU-21 — Warning, progress and log callbacks across the C ABI

**Warning, progress and log callbacks across the C ABI**

**Today.** no callback crosses the C ABI (`design/bindings-write.md:51-57`)

**Exit criterion.** each binding receives warnings and progress as they happen, in the order `Document::warnings` lists them

**Doc:** — · **Depends on:** SG-09

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-21.1 | build | Warning, progress and log callbacks on the C ABI, on SG-09.4's callback machinery, called in the order `Document::warnings` lists them. | `crates/tinker-pdf-ffi/src/lib.rs`, `crates/tinker-pdf-ffi/include/tinker_pdf.h`, `crates/tinker-pdf-ffi/tests/callbacks.rs` (new) | `callbacks.rs` (new): a fixture's warnings arrive as they happen, in `Document::warnings`' order, with its progress events and log lines. | SG-09.4 |
| SU-21.2 | build | Each binding over the callbacks: .NET, Go, Ruby, Java, Python and JavaScript. | `bindings/dotnet`, `bindings/go`, `bindings/java`, `bindings/ruby`, `bindings/python`, `bindings/js` | Each binding's test receives warnings and progress as they happen, in that order. | SU-21.1 |

**Closes the row:** SU-21.2 delivers warnings and progress to each binding as they happen in `Document::warnings`' order, which is the row's exit.

## SU-22 — Feature-gated builds

**Feature-gated builds** per format and capability (PDF only; each container; HTML and CSS; SVG; scripts; signing; OCR), each a footprint profile for MS-06

**Today.** `bundled-fonts`, `cmap-predefined` and a few others

**Exit criterion.** every gated build compiles, passes its own tests and reports its footprint

**Doc:** — · **Depends on:** MS-06

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-22.1 | build | Feature gates per format and capability: PDF only, each container, HTML and CSS, SVG, scripts, signing, OCR. | `Cargo.toml`, `crates/tinker-pdf/Cargo.toml`, `crates/tinker-pdf/src/lib.rs` | Each gated build compiles and passes its own tests. | — |
| SU-22.2 | ci | Each gated build a footprint profile in MS-06's ratchet. | `.github/workflows/ci.yml`, `bindings/js/footprint.json` (new) | CI builds every gated profile on `develop`, runs its tests, and reports its footprint against its committed figure. | SU-22.1, MS-06.3 |

**Closes the row:** SU-22.1 compiles and tests every gated build and SU-22.2 reports each one's footprint, which is the row's exit.

## SU-23 — A reference web viewer

**A reference web viewer** over the wasm build: navigation, search, selection, forms and scripts, annotations, layers, signatures panel, accessibility tree

**Today.** a demo renders an uploaded PDF in a browser (`release.yml` demo job)

**Exit criterion.** a browser-driven test opens each committed fixture and exercises each panel

**Doc:** — · **Depends on:** RC-15, AN-15

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-23.1 | build | The viewer shell over the wasm build: page navigation, zoom, thumbnails and the outline. | `bindings/js/viewer` (new), `bindings/js/tests` | A headless-browser test opens each committed fixture and navigates every page. | — |
| SU-23.2 | build | Search and selection. | `bindings/js/viewer` (new), `bindings/js/tests` | The test searches and selects on each fixture and gets the facade's matches and text. | SU-23.1 |
| SU-23.3 | build | Forms and scripts through AN-15's filling session. | `bindings/js/viewer` (new), `bindings/js/tests` | The test fills each form fixture through the panel and the saved values read back. | SU-23.1, AN-15 |
| SU-23.4 | build | Annotations, layers and the signatures panel. | `bindings/js/viewer` (new), `bindings/js/tests` | Each panel lists what the facade lists for each fixture. | SU-23.1 |
| SU-23.5 | build | The accessibility tree panel over SD-09. | `bindings/js/viewer` (new), `bindings/js/tests` | The panel lists each tagged fixture's tree as SD-09 gives it. | SU-23.1, SD-09 |
| SU-23.6 | build | Progressive rendering through RC-15. | `bindings/js/viewer` (new), `bindings/js/tests` | A large page renders in budgeted steps and the final pixels equal a single render's. | SU-23.1, RC-15.4 |
| SU-23.7 | test | The browser-driven test exercising each panel on each committed fixture, in CI. | `bindings/js/tests`, `.github/workflows/ci.yml` | On a CI run on `develop` the test opens each committed fixture and exercises each panel. | SU-23.2, SU-23.3, SU-23.4, SU-23.5, SU-23.6 |

**Closes the row:** SU-23.7 opens each committed fixture in a browser-driven test and exercises each panel, which is the row's exit.

## SU-24 — Desktop and mobile viewer applications

**Desktop and mobile viewer applications** over the bindings

**Today.** absent — the owner's parity decision of 9 October 2026; the only viewer is `release.yml`'s browser demo (SU-23)

**Exit criterion.** each application's smoke test opens, renders and searches the committed fixtures

**Doc:** `design/viewers.md` (owed) · **Depends on:** SU-23, SU-15

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SU-24.1 | design | Create `docs/design/viewers.md`: the platforms, the toolkit for each over a binding, what each application shares with SU-23's viewer, packaging, and milestones with smoke tests. | `docs/design/viewers.md` (new) | The doc exists in the design-doc order, names SU-24.2 to SU-24.5's exit tests, and is reviewed. | SU-23 |
| SU-24.2 | build | The desktop application for Linux, Windows and macOS: open, render, navigate and search. | `apps/desktop` (new), `.github/workflows/ci.yml` | Its smoke test opens, renders and searches the committed fixtures on each desktop CI leg. | SU-24.1 |
| SU-24.3 | build | The Android application over SU-15.2's AAR. | `apps/android` (new), `.github/workflows/ci.yml` | Its smoke test opens, renders and searches the committed fixtures on an emulator leg. | SU-24.1, SU-15.2 |
| SU-24.4 | build | The iOS application over SU-15.3's XCFramework. | `apps/ios` (new), `.github/workflows/ci.yml` | Its smoke test opens, renders and searches the committed fixtures on a simulator leg. | SU-24.1, SU-15.3 |
| SU-24.5 | watch | Each application's smoke test read on its CI leg. | `.github/workflows/ci.yml`, `docs/features/bindings.md` | On a CI run on `develop`, read to the end, each application opens, renders and searches the committed fixtures. | SU-24.2, SU-24.3, SU-24.4 |

**Closes the row:** SU-24.5 reads each application's smoke test opening, rendering and searching the committed fixtures, which is the row's exit.
