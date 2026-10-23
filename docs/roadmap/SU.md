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

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-02 — A lossless JSON form of a document

**A lossless JSON form of a document**: every object, streams inline or as files, the trailer and cross-reference facts, filters by key and object, a schema description; JSON back to PDF, creating or updating with object numbers kept

**Today.** `tpdf objects` dumps an object (`--object N [--stream [--raw]]`)

**Exit criterion.** every committed fixture round-trips PDF → JSON → PDF to an equal object graph; the schema validates every export; `tpdf json` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-03 — A text-editable normalised form

**A text-editable normalised form** — uncompressed, normalised, one object per line with original numbers as comments — and a fix-offsets pass so hand edits become valid again

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'fix_offsets|normali[sz]ed_form' crates tools` finds nothing

**Exit criterion.** a normalised fixture hand-edited in the test and fixed re-opens with the edit and validates

**Doc:** — · **Depends on:** ED-37

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-04 — A structural check with an exit-code contract

**A structural check with an exit-code contract**: categories (cross-reference, stream lengths, encryption, linearization and hint tables, object streams, page tree), documented exit codes, hint tables displayed

**Today.** `tpdf check --strict`

**Exit criterion.** each category's failure reached by a fixture and mapped to its documented exit code

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-05 — Inspection breadth

**Inspection breadth**: object paths and labels from the trailer, wildcard paths (`pages/*/Resources/Font/*/BaseFont`), cross-reference entries with object-stream indices, raw and decoded streams, encryption parameters, every script and URL, the structure tree with text, named destinations, every page box with paper-size names, effective image resolution per placement, font substitutions, a claimed-standards summary, per-page resource inventories

**Today.** `info`, `fonts`, `images`, `outline`, `objects`, `probe`

**Exit criterion.** each listing over the committed fixtures equals the facts walked independently in the test; `tpdf show`, `tpdf pages` and `tpdf info` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-06 — Caller-tunable parser limits and a no-repair mode

**Caller-tunable parser limits and a no-repair mode**: the compiled caps adjustable within their ledger bounds per document, and a strict mode that fails on the first defect instead of repairing

**Today.** 65 compiled caps (`bounds_ledger.rs`); `--strict` validates after the fact

**Exit criterion.** each tunable cap held by `bounds_ledger.rs` to a ceiling; strict mode fails on each repair class's fixture

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-07 — A space-usage audit

**A space-usage audit**: bytes by images, fonts, content, structure, forms, annotations, metadata, cross-reference data, thumbnails, attachments, unreferenced objects and old revisions, with the largest objects; operator frequencies

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'space_usage|space usage' crates tools` finds nothing

**Exit criterion.** the categories of each committed fixture sum to its length; `tpdf audit` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-08 — Write-determinism controls

**Write-determinism controls**: object-stream policy (keep, disable, generate), a deterministic `/ID` from content, static IDs for tests, a newline before `endstream`, reproducible output stated as a guarantee

**Today.** `/ID` is already a SHA-256 of the serialised document, an inherited `/ID[0]` kept (`crates/tinker-pdf-cos/src/write.rs:948-990`); `WriteOptions::object_streams` generates or does not, with no keep policy; no static-identifier switch for tests

**Exit criterion.** the same input and options give byte-identical output on four targets; each switch held to a fixture

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-09 — Version management

**Version management**: a minimum or forced version, `/Extensions` written, 2.0-only features converted down for 1.4–1.7 readers

**Today.** `with_version` on the builder

**Exit criterion.** down-converted fixtures pass the object grammar (SD-20) at the target version

**Doc:** — · **Depends on:** SD-20

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-10 — CLI conventions

**CLI conventions**: documented exit codes, `--json` on every reading verb with a schema, `-` for standard input and output, passwords from a file or the environment, one page-range grammar (ED-11), atomic in-place replacement, job files for reproducible batches, shell completion, `help <topic>`, progress on a terminal, quiet and verbose levels, directory batches with `--jobs`, a dry run for writers, `%d` in output names

**Today.** per-command flags

**Exit criterion.** a convention test suite over every verb

**Doc:** — · **Depends on:** ED-11

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-11 — `tpdf run`: scripts against the facade

**`tpdf run`: scripts against the facade** in the in-tree ECMAScript engine, with a REPL — the facade's object model exposed to scripts as a binding (ruling 11)

**Today.** absent — the owner's parity decision of 9 October 2026; `tpdf` has no `run` subcommand (`tools/tpdf/src/main.rs:291-308` lists eighteen)

**Exit criterion.** the parity script `bindings-parity` runs as a `tpdf run` script and prints the recorded hashes; `tpdf run` runs it on `testdata/` fixtures

**Doc:** `design/script-shell.md` (owed) · **Depends on:** FJ-03e, SU-12

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-12 — Every facade item on every surface

**Every facade item on every surface** (widened from the bindings row): the owed builder, editor, graphics-writing, tagged-writing, signature, public-key, timestamp, standalone-opener, `to_svg`, `display_list` and recoding projections, and the untracked ones — redaction, PDF/A, PDF/UA and PDF/X validation, search, structured-text writers, reading order and tables, font listing, `render_form`/`render_annotation`, layers, the subset report, `check_save` and `check_decrypt` — plus each row the roadmap adds; a generated coverage check

**Today.** **The C ABI exports 221 functions today, and the Go and Java bindings call every one of them** ([features/bindings.md](../features/bindings.md)). **Owed projections**: the rest of `PageBuilder` (`glyphs`, `text_pieces`, `shading`) and of `DocumentBuilder` (`add_cid_font`, `glyph_run`, `add_shading`) — the graphics states, form XObjects, tiling patterns and their page setters, `add_named_font` with `encoded_text`, `set_bleed_box`, `with_version` and `clear_image_resources` crossed in October 2026 — and the builder methods: `add_shading_pattern`, `add_icc_color_space` with `set_fill_icc` / `set_stroke_icc` and `ImageColorSpace::Icc`, the October 2026 CIE writer that is its twin (`CieSpace` with `add_cie_color_space`, `set_fill_cie` / `set_stroke_cie`, and `ImageColorSpace::Cie` and `ImageColorSpace::Lab`), `associate_file` on both builders with the October 2026 holders (`add_form_with_files`, `associate_file_with_structure`, `associate_file_with_link`, `with_associated_files`) and their readers (`associated_files_of`, `structure_associated_files`, `annotation_associated_files`, `marked_content_associated_files`), `PageBuilder::output_intent` and `link_for`, `finish_reporting` with the `EmbeddedWhole` list it returns, `dangling_destinations`, and the archival builder (`archival`, `profile`, `refusals`, `finish_archival`); `DocumentEditor`'s `import_page`, `keep_pages`, `flatten_annotations`, `add_annotation`, `reset_form`, `set_field_values`, `set_calculated_values`, `check_save` and `check_decrypt` with `SaveRefusal` (the save doors' behaviour is H-10's), and of the document operations `set_viewer_preferences` (the rest — page labels, attachments, the outline, the typed `/Info` setters, XMP, the page boundaries and `sanitise` with its report — crossed in October 2026); `WriteOptions::deduplicate_streams` (the C ABI builds it at its default); the September 2026 graphics-writing surface — `Target::Named` and both `add_named_destination`s, `DocumentBuilder::add_layer` with `PageBuilder::optional` and `DocumentEditor::set_layer_visible`, `add_separation_color_space` and `add_device_n_color_space` with `Function::Calculator` and `DeviceNAttributes`, `set_fill_tint` / `set_stroke_tint` and `ImageColorSpace::Tint`, `DocumentEditor`'s `stamp`, `add_resource`, `add_form` and `import_page_as_form`, and `Page::images` with `PageImage`; of the forms surface, `SigningTarget::NewVisibleField` with `SignatureAppearance` (a signing target, so it waits on signing crossing at all, SG-09) and the annotation payloads with `Page::annotation_list` (the rest — `DocumentEditor::add_field` and `NewField`, and `form_data`'s FDF and XFDF reading, writing and applying — crossed in October 2026); of tagged writing, a tag's `table`, `namespace` and `associated_file`, `continue_at`, `map_role_in`, `add_namespace` and `duplicate_element_ids` (`PageBuilder::tagged` as the `open_tag`/`close_tag` pair, with `set_language` and `map_role`, crossed in October 2026); a signature's `/Contents`, `/M`, `/ContactInfo`, `/Filter` and warnings through the C ABI (Python and JavaScript carry them), and `modifications` on every surface; `authenticate_with_recipient`; the October 2026 signature and public-key surface — `DocumentEditor::save_timestamped` with `Timestamper` and `TimestampRequest`, `add_validation_data` with `ValidationData`, `Document::security_store` with `SecurityStore` and its warnings, `PublicKeyEncryption::seal` with `EntropySource` and `DocumentEditor::save_sealed`, `Verdict::timestamps` with `TimestampVerdict`, and `Signature::validation_key`; and the wave 4–6 openers and outputs — the standalone-document openers (`Document::open_with` and `sniff`), `FromHtml`, `Document::open_markdown` and FB2, `Page::to_svg` and `DisplayList::to_svg` with `SvgOptions`, `Page::display_list` (retained pages) and image recompression (`recode`). The read half of the owed list has no section in [design/bindings-write.md](../design/bindings-write.md); the ten facade areas named last in the item reach no binding and no row tracked them before this one (a grep of the C header against the facade, 9 October 2026)

**Exit criterion.** each through the C ABI and every wrapper over it — .NET, Go, Ruby, Java — and Python and JavaScript, with a parity-script line; `cargo xtask bindings-coverage` lists every public facade item against the C ABI and each binding and fails on a gap not named in `features/bindings.md`'s refusal table; `cargo xtask bindings-parity` green

**Doc:** [design/bindings-write.md](../design/bindings-write.md) (read half added) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-13 — A Swift binding that has been built; Ruby's streaming open

**A Swift binding that has been built; Ruby's streaming open**

**Today.** **Go, Ruby and Java landed, October 2026**, in that order, each a thin wrapper over the C header with a smoke program and a parity program, wired into `cargo xtask bindings-parity` and `ci.yml`'s `bindings-parity` job, and each printing all thirteen recorded hashes on linux/x86_64; Go (cgo) and Java (FFM, JDK 21 with `--enable-preview`, flagless from 22 — the 22 half unverified, H-105) call all 221 C functions, Ruby (Fiddle) 220, refusing the streaming open by name because Fiddle passes no struct by value. **Swift is source only**: a SwiftPM package over the committed header covering 49 functions, never compiled, because no Swift toolchain was available ([features/bindings.md](../features/bindings.md))

**Exit criterion.** Swift built on a macOS CI leg, its smoke run and a parity program printing the recorded hashes, wired into `bindings-parity` and CI, and widened to the header; Ruby's streaming open through a by-pointer vtable on the C ABI with callbacks that are safe off the Ruby thread, or kept refused by name

**Doc:** — · **Depends on:** H-103

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-14 — Binding ergonomics

**Binding ergonomics**: typed exceptions, Promise-returning APIs and a worker helper in JavaScript, cancellation and progress everywhere, stream adapters (file-like objects, Node streams, .NET `Stream`, Java `InputStream`) over `ByteSource`, and a CommonJS entry that re-exports the one ESM build

**Today.** ESM only; a CommonJS build refused because "two builds of the engine can diverge" (`features/bindings.md:1051`)

**Exit criterion.** each binding's test suite exercises each shape; the CommonJS entry loads the same wasm bytes as the ESM one (hash checked)

**Doc:** — · **Depends on:** SU-12

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-15 — More platforms

**More platforms**: a C++ header-only wrapper over the C ABI; Android through JNI and an AAR per ABI; an iOS XCFramework

**Today.** C ABI, .NET, Go, Ruby, Java (FFM), Python, JavaScript

**Exit criterion.** each prints the recorded hashes in `bindings-parity` on its platform's CI leg

**Doc:** — · **Depends on:** SU-13

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-16 — A high-level convenience layer per binding

**A high-level convenience layer per binding**: one-call open, render, extract, fill, sign, convert and save helpers mirroring facade presets

**Today.** the bindings project the facade 1:1

**Exit criterion.** each helper's output equals the long-hand facade sequence's (byte hash)

**Doc:** — · **Depends on:** SU-12

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-17 — Packaging, the machine half

**Packaging, the machine half**: crates.io and docs.rs (a publishing policy for leaf crates), wheels for manylinux and musllinux (x86-64 and aarch64), macOS universal2, Windows x64 and arm64 with type stubs, npm with a SIMD variant and the worker helper, NuGet with every runtime identifier, Maven Central classifiers and an FFM variant, a Go module with prebuilt static libraries, native gems, a SwiftPM XCFramework, recipes for conda, Homebrew, the major distributions, a container image and winget, pkg-config and CMake files, vcpkg and Conan ports, a versioned shared object

**Today.** nothing published: `pip install`, `npm install` and `dotnet add package` do not work, by decision, until the facade freezes at 0.1.0 ([features/bindings.md](../features/bindings.md)); `release.yml` builds crates, wheels, npm, natives and NuGet, has run once and published nothing

**Exit criterion.** a release dry run (`workflow_dispatch` without `publish`) builds and installs every artefact on its platform and renders a page

**Doc:** — · **Depends on:** H-113, H-115

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-18 — API stability, checked

**API stability, checked**: a semver diff of the facade in CI, versioned C structures with size fields and a versioned shared object, a deprecation window, an MSRV policy, a changelog, experimental markers

**Today.** "the facade freezes at 0.1.0"; nothing checks a break

**Exit criterion.** the CI diff fails on an unannounced break; every C structure carries its size; the MSRV built in CI

**Doc:** — · **Depends on:** H-114, H-16 (tool allowlist)

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-19 — The documentation set

**The documentation set**: rustdoc examples everywhere; a guide and a task cookbook ("fill a form", "sign with a key file", "make PDF/A"); per-binding references; runnable examples per binding in CI; manual pages; a catalogue of every typed warning with its meaning and remedy; a browser playground on the wasm build

**Today.** feature docs and design docs; seven examples run in CI

**Exit criterion.** every example compiles and runs in CI; the warning catalogue is generated from the source and fails on an undocumented variant

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-20 — Supply-chain evidence

**Supply-chain evidence**: an SBOM per artefact, build provenance, signed releases, attestations on publishing, a reproducible-build check

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'sbom|cyclonedx|attest' .github` finds nothing

**Exit criterion.** the release dry run emits each and a second build reproduces the artefacts' hashes

**Doc:** — · **Depends on:** SU-17

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-21 — Warning, progress and log callbacks across the C ABI

**Warning, progress and log callbacks across the C ABI**

**Today.** no callback crosses the C ABI (`design/bindings-write.md:51-57`)

**Exit criterion.** each binding receives warnings and progress as they happen, in the order `Document::warnings` lists them

**Doc:** — · **Depends on:** SG-09

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-22 — Feature-gated builds

**Feature-gated builds** per format and capability (PDF only; each container; HTML and CSS; SVG; scripts; signing; OCR), each a footprint profile for MS-06

**Today.** `bundled-fonts`, `cmap-predefined` and a few others

**Exit criterion.** every gated build compiles, passes its own tests and reports its footprint

**Doc:** — · **Depends on:** MS-06

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-23 — A reference web viewer

**A reference web viewer** over the wasm build: navigation, search, selection, forms and scripts, annotations, layers, signatures panel, accessibility tree

**Today.** a demo renders an uploaded PDF in a browser (`release.yml` demo job)

**Exit criterion.** a browser-driven test opens each committed fixture and exercises each panel

**Doc:** — · **Depends on:** RC-15, AN-15

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SU-24 — Desktop and mobile viewer applications

**Desktop and mobile viewer applications** over the bindings

**Today.** absent — the owner's parity decision of 9 October 2026; the only viewer is `release.yml`'s browser demo (SU-23)

**Exit criterion.** each application's smoke test opens, renders and searches the committed fixtures

**Doc:** `design/viewers.md` (owed) · **Depends on:** SU-23, SU-15

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*
