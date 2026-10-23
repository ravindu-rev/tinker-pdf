# Capability tier — annotations, forms and XFA

[The roadmap](../ROADMAP.md) says how a step runs, what comes next, and which dependencies cross tiers. Each row below keeps its text as it was planned — Item, Today, Exit criterion, Doc, Depends on — and is cut into steps, each one workflow run.

Parity here means every annotation subtype created, edited, drawn and
exchanged; every field type created and filled interactively; every action
typed; and XFA — removed from ISO 32000-2 but carried by deployed forms,
and read, laid out, scripted and flattened by the field — brought in as
rows. Form JavaScript has its own tier, next.

## AN-01 — Per-subtype annotation payloads

**Per-subtype annotation payloads** — the measurement

**Today.** **The payloads landed; the corpus count per family is owed.** `Annotation::payload` is an `AnnotationPayload`, one variant per 12.5.6 family (23, and `None` for an untyped entry), each carrying its table's entries with its table's defaults — `/QuadPoints`, `/InkList`, `/Vertices`, `/L` with `/LE`, `/LL`, `/LLE`, `/LLO`, `/DA`, `/IC`, `/CL`, `/BE`, `/RD`, `/FS` as a file specification, `/Open`, `/State`/`/StateModel`, `/Name` icons, `/Sound`, `/Movie`, `/3DD` and rich media by reference — beside Table 164's and 170's common entries, now including `/C`, `/BS`/`/Border`, `/CA`, `/RC`, `/Subj`, `/CreationDate`, `/IRT`, `/RT`, `/IT`, `/NM` and `/AS`; `carries_required()` says whether a family's required entries are there. Every family is held to a hand-written fixture in `crates/tinker-pdf/src/annotations.rs`. **The cap names what it dropped without mutating anything**: `Page::annotation_list()` returns an `AnnotationList` whose `dropped` counts the `/Annots` entries past 4 096, and a second bound, `MAX_ANNOTATION_BYTES` (64 MiB of copies a page, a `bounds_ledger.rs` row), closes an amplification the listing had from the start — one shared object named by 4 096 entries was copied 4 096 times — and is counted the same way in `incomplete`. What is left is the measurement: `crates/tinker-pdf/tests/annotation_census.rs` counts payloads per family and runs nightly in `corpus.yml` from 26 September 2026, but the fetched corpora were not reachable where it was written, so **no per-family count has been taken** and the census holds no floor on one. The census before this row: 14 996 annotations on 982 files, 14 986 covered, 10 refused across 6 subtype names

**Exit criterion.** the per-family counts from the census's first nightly run, recorded here and held as floors

**Doc:** — · **Depends on:** MS-13

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-02 — Appearances for the subtypes no dictionary determines

**Appearances for the subtypes no dictionary determines** (redaction-and-editing clause (d)): `Stamp` (the fourteen standard stamp names and image stamps), `FileAttachment` and `Sound` icons, `Popup`, `Redact` (overlay), `Watermark`, `Screen`/`Movie`/`3D`/`RichMedia` posters, `PrinterMark`, `TrapNet`, `Projection`; and `Text`'s seven icon names

**Today.** **Appearance synthesis draws thirteen subtypes and declines fourteen; the declining is not ruled.** October 2026 added `Line`, `Polygon`, `PolyLine`, `Squiggly`, `Caret`, `Ink` and `FreeText` beside `Highlight`, `Underline`, `StrikeOut`, `Square`, `Circle` and `Text` (`Link` draws none), and `/RD`, dashes and `/CA` for the first seven — whose bytes did not move only for a well-formed dictionary carrying none of `/CA`, `/ca`, `/RD`, a `/BS /S /D` or a `/Border` dash, which is what the editor's constructors make; with one they draw what it says, and with a malformed entry every subtype is declined. **The fourteen subtypes `UNDETERMINED_SUBTYPES` names get no `/AP`** (`add_annotation`, `features/editing.md:831`, `crates/tinker-pdf-cos/src/appearance.rs`) — the implementing lane's position, argued in the feature doc's table, that no dictionary determines their appearance, which it does not apply evenly: `Text` is drawn as an invented note though 12.5.6.4 leaves its `/Name` icon to the reader exactly as 12.5.6.12, .15 and .16 leave `Stamp`'s, `FileAttachment`'s and `Sound`'s, and `Caret`'s outline is invented too, so whether those three get invented icons or an invented icon is refused (and `Text`'s with it) is the owner's (H-12)

**Exit criterion.** an `/AP` per further subtype, or the owner's ruling on which of the fourteen (and `Text`'s invented note) are refused; each appearance drawn is a reviewed golden family (K-08, H-66), and the dictionary entries that size and colour it are held to fixtures

**Doc:** — · **Depends on:** H-12

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-03 — Free text completeness

**Free text completeness**: a composite `/DA` font; callout lines (`/CL`, `/LE`), quadding, the typewriter intent

**Today.** a free text annotation whose `/DA` names a composite font is left bare — a field's value is shaped in one (`fill.rs`'s `Composite`) and a free text annotation's is not — as is one whose font `/DR` lacks, a symbolic one, or one whose encoding has no byte for a character (`features/editing.md:829`)

**Exit criterion.** a free text annotation in a composite `/DA` font given an `/AP` whose text renders where its layout puts it; fixtures per case render and extract as written

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-04 — Border effects and styles

**Border effects and styles**: cloudy `/BE` with intensity, beveled, inset and underline `/BS`, every dash pattern, a `/Border`'s corner radii

**Today.** a cloudy `/BE`, the beveled, inset and underline `/BS` styles and a `/Border`'s corner radii are drawn solid and square, each a row of the feature doc's refusal table (`features/editing.md:835`)

**Exit criterion.** the cloud's arc geometry and the bevel shading computed in the test; reviewed goldens for the invented looks (H-66); or the owner's ruling that they are refused

**Doc:** — · **Depends on:** H-12

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-05 — Line captions and leaders; a caret's paragraph symbol

**Line captions and leaders; a caret's paragraph symbol**

**Today.** a line's caption (`/Cap`) is not drawn and a caret's paragraph symbol (`/Sy /P`) is not drawn, each a row of the feature doc's refusal table (`features/editing.md:832-833`)

**Exit criterion.** caption placement computed from `/CP` and `/CO` in the test; leader geometry from `/LL`, `/LLE`, `/LLO`; the ¶ drawn, or the owner's ruling that it is refused

**Doc:** — · **Depends on:** H-12

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-06 — Annotation editing, complete

**Annotation editing, complete**: create every markup subtype with every property; vertices, ink strokes and quad points added and cleared; line endings; intents; popups and open state; reply (`/IRT`, `/RT`) set and replies deleted; rotation, optional content and blend; an update cycle that regenerates stale appearances; appearances set from a display list; rollover and down states; synthesised appearances kept out of the file until saved; links updated and deleted

**Today.** `add_annotation`; constructors `highlight`, `square`, `text_note`, `link`; `flatten_annotations`

**Exit criterion.** each property written, re-read and drawn; the update cycle regenerates exactly the annotations whose dictionaries changed; `tpdf annots list` and `remove` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-07 — The review model

**The review model**: reply threads, per-user review states (accepted, rejected, cancelled, completed; marked, unmarked), filtering and sorting, a comment-summary document, migrating comments between versions

**Today.** payloads read; no semantics

**Exit criterion.** fixtures with known threads and states round-trip; the summary lists every comment once

**Doc:** — · **Depends on:** AN-06

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-08 — Annotation exchange through XFDF and FDF

**Annotation exchange through XFDF and FDF**: every markup subtype with appearance data, popups, replies, states, rich text, ink, attachments, page mapping, and delta export

**Today.** XFDF and FDF annotations are `FormDataWarning::NotRead` (`features/forms.md:556`)

**Exit criterion.** export then import reproduces every annotation dictionary; documents exchanged with a generated set match; `tpdf annots export` and `import` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** H-102 (ISO 19444-1)

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-09 — Measurement

**Measurement**: `/Measure` rectilinear dictionaries (scale ratio and number formats), viewports `/VP`, dimension intents with captions; distances, areas and perimeters computed

**Today.** line captions refused; `/Measure` not read

**Exit criterion.** a drawing with a known scale yields the known lengths and areas

**Doc:** — · **Depends on:** AN-05

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-10a — Geospatial data read and written

**Geospatial data read and written**: `/Measure /GEO`, `/GCS`, `/GPTS`, `/LPTS`, `/PCSM`, `/DCS`, `/PDU`, and 2.0's `/PtData`

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -w 'GPTS|LPTS|PtData|GCS' crates` finds nothing

**Exit criterion.** fixtures round-trip every key; a page point maps to the registered point list by the dictionary's affine

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-10b — Geospatial projection arithmetic

**Geospatial projection arithmetic**: page point to latitude and longitude through the stated coordinate system

**Today.** absent — the owner's parity decision of 9 October 2026; latitude and longitude appear only as XMP property names the archival validator types (`crates/tinker-pdf/src/pdfa/xmp_schemas.rs`)

**Exit criterion.** published projection examples (the projection standards' worked numbers) reproduced in the test

**Doc:** `design/geospatial.md` (owed) · **Depends on:** AN-10a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-11a — 3D assets

**3D assets**: U3D (ECMA-363) and PRC (ISO 14739-1) parsed, views (`/3DV`) read, models exported as meshes

**Today.** the `/3DD` payload is read by reference only (AN-01)

**Exit criterion.** the format specifications' sample structures parsed; exported meshes' vertex and face counts equal the stated ones

**Doc:** `design/3d.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-11b — 3D views drawn into the page

**3D views drawn into the page** by a software rasteriser (camera, render mode, lighting, cross-sections, node visibility)

**Today.** absent — the owner's parity decision of 9 October 2026; a `/3D` stream is carried by reference (AN-01) and nothing parses U3D or PRC: `rg -i 'u3d' crates` finds fixtures and the archival rule that names the subtype

**Exit criterion.** analytic scenes (a cube under a stated camera) render to the projections computed in the test; determinism on four targets

**Doc:** `design/3d.md` (owed) · **Depends on:** AN-11a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-11c — The 3D scripting API

**The 3D scripting API**

**Today.** absent — the owner's parity decision of 9 October 2026; no 3D content is parsed (AN-11b's search)

**Exit criterion.** the object model's stated behaviours held to expectations a person wrote (H-70)

**Doc:** `design/3d.md` (owed) · **Depends on:** AN-11b, FJ-11, H-70

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-12 — The multimedia model

**The multimedia model**: Screen annotations, rendition actions, media clips with permissions, Sound annotations decoded (raw, signed, µ-law, A-law) to PCM, Movie, 2.0 RichMedia assets and configurations — extracted and exposed for a host to play

**Today.** payload by reference; no appearance synthesis for these subtypes

**Exit criterion.** a sound's samples decoded by the codec formulas in the test; assets extracted byte-equal

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-13 — Flattening policies

**Flattening policies**: all, print-visible or screen-visible annotations; widgets separately; signatures kept

**Today.** `flatten_annotations`

**Exit criterion.** each policy's survivors match the flags; the render under each usage is unchanged; `tpdf annots flatten`, `tpdf forms flatten` and `tpdf bake` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** RC-13

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-14 — Every field type created

**Every field type created**: push buttons with captions, icons and layout; list boxes; empty signature fields; file-select, password, rich-text and comb fields; appearance characteristics (`/MK`) colours; unique names generated from a prefix

**Today.** text, checkbox, radio and choice fields created (`features/forms.md:16-191`)

**Exit criterion.** each type created, filled through the editor and re-read; appearances drawn and validated

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-15 — An interactive form-filling session

**An interactive form-filling session**: focus and tab order (`/Tabs`), key and character input routed through keystroke events, caret and selection, input-method-safe replacement, per-field undo and redo, editable combo boxes, multi-select lists, button toggling (radios in unison, no toggle to off), password masking, file selection, comb and multi-line scrolling, highlight colour, hit testing, invalidated rectangles, text direction per field

**Today.** fill by API only (`fill_field`, `keystroke`)

**Exit criterion.** a scripted session over the committed form fixtures produces the field values and appearances a direct fill produces; invalidated rectangles cover exactly the changed pixels

**Doc:** `design/form-session.md` (owed) · **Depends on:** FJ-06

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-16 — Rich text in free text and fields

**Rich text in free text and fields**: `/RC`, `/RV` and `/DS` laid out by the CSS and layout engine

**Today.** `/RC` rich text and `/DS` default style are drawn as plain `/Contents` (`features/editing.md:830`)

**Exit criterion.** a free text annotation with `/RC` and `/DS` given an `/AP` whose text renders where its layout puts it; each rich-text fixture renders as the same XHTML laid out by the EPUB path at that box

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-17 — Signature fields through the form layer

**Signature fields through the form layer**: create one empty, sign it (through SG rows), clear it

**Today.** refused: "Filling or signing a signature field" (`features/forms.md:572`)

**Exit criterion.** a field created, signed and verified through the form layer gives the verdict the signing path gives

**Doc:** — · **Depends on:** SG-06

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-18 — Page templates

**Page templates** (12.7.6): the `/Templates` tree, spawning with field renaming, hidden templates

**Today.** absent — the owner's parity decision of 9 October 2026; `/Templates` is named only in a comment of the FDF reader (`crates/tinker-pdf/src/form_data.rs:27`); nothing spawns a page

**Exit criterion.** a spawned page's fields are named by the 12.7.6 scheme and fill independently

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-19 — Submit, reset and import as payloads

**Submit, reset and import as payloads**: every submit flag (include/exclude, empty fields, HTML-form, FDF, XFDF or PDF format, GET, coordinates, incremental appends, annotations, canonical dates) producing the exact bytes, URL and method for the host to send; reset with field lists; import data

**Today.** FDF and XFDF read, written and applied

**Exit criterion.** each flag combination's payload equals the bytes computed in the test from 12.7.6.2's rules

**Doc:** — · **Depends on:** AN-20

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-20 — Every action typed

**Every action typed**: `/JavaScript`, `/SubmitForm`, `/ResetForm`, `/ImportData`, `/Hide`, `/SetOCGState`, `/Rendition`, `/Trans`, `/GoTo3DView`, `/GoToE`, `/GoToDp`, `/Thread`, `/Sound`, `/Movie`, `/RichMediaExecute`; chains through `/Next` with cycle detection; each handed to the host as a typed request, the engine executing none that needs the outside world

**Today.** `Action` has `GoTo`, `GoToR`, `Uri`, `Named`, `Launch` and `Other` (`crates/tinker-pdf-cos/src/dest.rs:88-114`; `features/document-model.md:433`)

**Exit criterion.** a fixture per action type read into its arm; `Hide`, `SetOCGState` and `ResetForm` applied by the engine and held to their results

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-21 — Batch filling and forms from data

**Batch filling and forms from data**: fill a template from many CSV or JSON rows into many outputs or one; create a form from a JSON description

**Today.** FDF/XFDF apply and `add_field`

**Exit criterion.** N rows produce N documents whose field values equal the rows; `tpdf forms fill` and `reset` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** AN-14

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-22 — Form data export and aggregation

**Form data export and aggregation**: every field with type, options and flags to CSV, JSON or XML; many returned copies merged into one table

**Today.** `fields` listing; FDF/XFDF export

**Exit criterion.** export re-imported reproduces the values; aggregation of generated copies equals the generated table; `tpdf forms list` and `export` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-23 — The rest of FDF and XFDF

**The rest of FDF and XFDF**: page templates, JavaScript, appearances, flags and rich text; encrypted FDF

**Today.** `FormDataWarning::NotRead`; `FormDataError::Encrypted` (`features/forms.md:556-557`)

**Exit criterion.** every key read and applied; an encrypted FDF opened with its password

**Doc:** — · **Depends on:** AN-08

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-24 — Barcode fields for paper forms

**Barcode fields for paper forms**: a field drawing a PDF417, QR or Data Matrix encoding of other fields' values, updated by a calculation

**Today.** absent — the owner's parity decision of 9 October 2026; no symbology is encoded (ED-49's search)

**Exit criterion.** the drawn symbol decodes (ED-50) to the values after every recalculation

**Doc:** — · **Depends on:** ED-49, ED-50, FJ-07

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-25 — Form-field detection on flat forms

**Form-field detection on flat forms**: boxes, underlines, comb cells, checkboxes and their labels turned into named fields

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'detect_fields|field_detection' crates tools` finds nothing

**Exit criterion.** generated flat forms with known fields detected with recall and precision ≥ the floor the design states

**Doc:** `design/form-detection.md` (owed) · **Depends on:** FT-30

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-26 — Spell checking of fields and comments

**Spell checking of fields and comments** with host-supplied dictionaries

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'spellcheck|spell_check' crates tools` finds nothing

**Exit criterion.** a host dictionary flags exactly the misspellings planted in fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-27 — XFA read

**XFA read**: the packets (template, datasets, config, localeSet, connectionSet, form, xfdf, sourceSet, stylesheet, signature) from an array or stream and stand-alone XDP; "pure XFA" detected; datasets exported and imported; XFA removed to fall back to AcroForm

**Today.** XFA was a named hard limit of the roadmap until 9 October 2026 — "removed in ISO 32000-2" (`features/forms.md:573`) — which is a reason about a standard and not about this repository's power: the specification is published free and deployed forms carry it; `sanitise` removes `/XFA` under `javascript`

**Exit criterion.** every packet of a generated XFA document read byte-equal; removal leaves a valid AcroForm document

**Doc:** `design/xfa.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-28 — Static XFA

**Static XFA**: AcroForm widgets and XFA data kept in step when filling and saving

**Today.** absent — the owner's parity decision of 9 October 2026; XFA is only removed (`Sanitise`'s `XfaForm`, `crates/tinker-pdf-cos/src/edit/sanitise.rs:136-137`) and named by the archival and accessibility validators; no packet is read

**Exit criterion.** a fill through either model is visible in the other after save and reopen

**Doc:** `design/xfa.md` (owed) · **Depends on:** AN-27

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-29 — The dynamic XFA layout engine

**The dynamic XFA layout engine**: template, data and form merge (data binding, SOM), flowed subforms (positioned, top-to-bottom, left-to-right, tables), pagination (page sets, page and content areas, occurrence, overflow leaders and trailers, keeps and breaks), draws, fields, exclusion groups, rich text, typeface mapping, barcode and image fields, instance managers

**Today.** absent — the owner's parity decision of 9 October 2026; no XFA packet is read (AN-28); `rg -i 'subform' crates` finds nothing

**Exit criterion.** layouts of the XFA specification's own examples reproduce its stated geometry, computed in the test; generated forms paginate deterministically on four targets; real government forms are a person's review (H-79)

**Doc:** `design/xfa.md` (owed) · **Depends on:** AN-27, CD-01

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-30 — The XFA scripting object model

**The XFA scripting object model**: `xfa.host`, `form`, `layout`, `event`, `datasets`, `resolveNode(s)`, instance managers, `$record`, raw and formatted values, presence, mandatory and access

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'resolveNode|xfa\.host' crates tools` finds nothing

**Exit criterion.** expectations a person wrote (H-70) from the specification's script examples

**Doc:** `design/xfa.md` (owed) · **Depends on:** AN-29, FJ-03e, H-70

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-31 — FormCalc

**FormCalc**: grammar, SOM expressions and the built-in functions (arithmetic, date and time, financial, logical, string, URL calls as host requests)

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'formcalc' crates tools` finds nothing

**Exit criterion.** each built-in held to the XFA specification's stated examples; the grammar fuzzed (FJ-05)

**Doc:** `design/xfa.md` (owed) · **Depends on:** FJ-01

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-32 — XFA picture clauses and locales

**XFA picture clauses and locales**: dates, times, numbers, text, zero and null patterns per locale set

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'picture_clause|picture clause' crates tools` finds nothing

**Exit criterion.** the specification's picture-clause examples formatted and parsed in the test

**Doc:** `design/xfa.md` (owed) · **Depends on:** AN-27

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-33 — XFA flattened to static PDF

**XFA flattened to static PDF**, optionally with AcroForm fields, for archiving and printing

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'xfa_flatten|flatten_xfa' crates tools` finds nothing

**Exit criterion.** the flattened document renders as the dynamic layout drew it and passes PDF/A where asked

**Doc:** `design/xfa.md` (owed) · **Depends on:** AN-29

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## AN-34 — XFA signatures

**XFA signatures** over data and template, created with in-engine keys and verified

**Today.** absent — the owner's parity decision of 9 October 2026; XFA's `signature` packet is not read (AN-27)

**Exit criterion.** XML signatures the engine creates over generated packets with SG-01's and SG-02's keys verify through SG-13's verifier, and a packet changed after signing is reported

**Doc:** `design/xfa.md` (owed) · **Depends on:** SG-13, AN-27

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*
