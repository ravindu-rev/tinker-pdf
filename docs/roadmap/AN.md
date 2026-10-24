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

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-01.1 | watch | Dispatch `corpus.yml` on `develop` and read `annotation_census.rs`'s per-family payload counts to the end in every corpus. | `.github/workflows/corpus.yml`, `crates/tinker-pdf/tests/annotation_census.rs` | The census prints its `RAN` line and every family's count in a green run on `develop`; the run's id and figures are quoted in AN-01.2's commit. | MS-13, MS-11, H-03 |
| AN-01.2 | measure | Hold the per-family counts as floors in the census, and record them in the roadmap row's place in the feature doc. | `crates/tinker-pdf/tests/annotation_census.rs`, `docs/features/editing.md` | The census fails when a family's count falls below its committed floor (a counted injection dropping one family's payload fires it) and passes at AN-01.1's figures; the feature doc quotes them with the run. | AN-01.1 |

**Closes the row:** AN-01.1 takes the census's first nightly figures and AN-01.2 records and holds them as floors, which is the row's exit.

## AN-02 — Appearances for the subtypes no dictionary determines

**Appearances for the subtypes no dictionary determines** (redaction-and-editing clause (d)): `Stamp` (the fourteen standard stamp names and image stamps), `FileAttachment` and `Sound` icons, `Popup`, `Redact` (overlay), `Watermark`, `Screen`/`Movie`/`3D`/`RichMedia` posters, `PrinterMark`, `TrapNet`, `Projection`; and `Text`'s seven icon names

**Today.** **Appearance synthesis draws thirteen subtypes and declines fourteen; the declining is not ruled.** October 2026 added `Line`, `Polygon`, `PolyLine`, `Squiggly`, `Caret`, `Ink` and `FreeText` beside `Highlight`, `Underline`, `StrikeOut`, `Square`, `Circle` and `Text` (`Link` draws none), and `/RD`, dashes and `/CA` for the first seven — whose bytes did not move only for a well-formed dictionary carrying none of `/CA`, `/ca`, `/RD`, a `/BS /S /D` or a `/Border` dash, which is what the editor's constructors make; with one they draw what it says, and with a malformed entry every subtype is declined. **The fourteen subtypes `UNDETERMINED_SUBTYPES` names get no `/AP`** (`add_annotation`, `features/editing.md:831`, `crates/tinker-pdf-cos/src/appearance.rs`) — the implementing lane's position, argued in the feature doc's table, that no dictionary determines their appearance, which it does not apply evenly: `Text` is drawn as an invented note though 12.5.6.4 leaves its `/Name` icon to the reader exactly as 12.5.6.12, .15 and .16 leave `Stamp`'s, `FileAttachment`'s and `Sound`'s, and `Caret`'s outline is invented too, so whether those three get invented icons or an invented icon is refused (and `Text`'s with it) is the owner's (H-12)

**Exit criterion.** an `/AP` per further subtype, or the owner's ruling on which of the fourteen (and `Text`'s invented note) are refused; each appearance drawn is a reviewed golden family (K-08, H-66), and the dictionary entries that size and colour it are held to fixtures

**Doc:** — · **Depends on:** H-12

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-02.1 | doc | Record H-12's ruling in `docs/rulings.md` and the feature doc's refusal table: which of the fourteen `UNDETERMINED_SUBTYPES` and `Text`'s invented note are drawn, and which are refused by name. | `docs/rulings.md`, `docs/features/editing.md`, `crates/tinker-pdf-cos/src/appearance.rs` | The ruling is recorded with its date; `UNDETERMINED_SUBTYPES` and the table name exactly the subtypes still refused. | H-12 |
| AN-02.2 | build | `Stamp` appearances: the fourteen standard stamp names drawn by a stated design, and image stamps drawn from their image. *Note:* Only the subtypes H-12 rules drawn. | `crates/tinker-pdf-cos/src/appearance.rs`, `crates/tinker-pdf/tests/annotation_appearances.rs` | `annotation_appearances.rs` `stamps_are_drawn`: each standard name and an image stamp get an `/AP` sized and coloured by its dictionary as fixtures hold; the family is a K-08 golden awaiting H-66. | AN-02.1, K-08 |
| AN-02.3 | build | Icons: `FileAttachment`'s and `Sound`'s, and `Text`'s seven icon names. *Note:* Only the subtypes H-12 rules drawn. | `crates/tinker-pdf-cos/src/appearance.rs`, `crates/tinker-pdf/tests/annotation_appearances.rs` | `icons_are_drawn`: each icon gets an `/AP` coloured by `/C` and sized by `/Rect`; the family is a K-08 golden awaiting H-66. | AN-02.1, K-08 |
| AN-02.4 | build | `Popup`, `Redact` overlays and `Watermark`. *Note:* Only the subtypes H-12 rules drawn. | `crates/tinker-pdf-cos/src/appearance.rs`, `crates/tinker-pdf/tests/annotation_appearances.rs` | `popups_redact_overlays_and_watermarks_are_drawn`: each gets an `/AP` from its dictionary's entries as fixtures hold; goldens awaiting H-66. | AN-02.1, K-08 |
| AN-02.5 | build | Posters for `Screen`, `Movie`, `3D` and `RichMedia`, and `PrinterMark`, `TrapNet` and `Projection`. *Note:* Only the subtypes H-12 rules drawn. | `crates/tinker-pdf-cos/src/appearance.rs`, `crates/tinker-pdf/tests/annotation_appearances.rs` | `posters_and_print_marks_are_drawn`: each gets an `/AP` as fixtures hold; goldens awaiting H-66. | AN-02.1, K-08 |
| AN-02.6 | watch | The new golden families read and signed by a person. | `crates/tinker-pdf/tests/render_goldens.rs` | Every family AN-02.2 to AN-02.5 added is signed as reviewed in its header under H-66. | AN-02.2, AN-02.3, AN-02.4, AN-02.5, H-66 |

**Closes the row:** AN-02.1 records which subtypes are refused, and AN-02.2 to AN-02.6 draw the rest as reviewed golden families sized and coloured by fixtures, the row's two exits.

## AN-03 — Free text completeness

**Free text completeness**: a composite `/DA` font; callout lines (`/CL`, `/LE`), quadding, the typewriter intent

**Today.** a free text annotation whose `/DA` names a composite font is left bare — a field's value is shaped in one (`fill.rs`'s `Composite`) and a free text annotation's is not — as is one whose font `/DR` lacks, a symbolic one, or one whose encoding has no byte for a character (`features/editing.md:829`)

**Exit criterion.** a free text annotation in a composite `/DA` font given an `/AP` whose text renders where its layout puts it; fixtures per case render and extract as written

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-03.1 | build | Free text in a composite `/DA` font, a font `/DR` lacks, a symbolic font, or an encoding with no byte for a character, shaped and drawn through `fill.rs`'s composite path as a field's value is. | `crates/tinker-pdf-cos/src/appearance.rs`, `crates/tinker-pdf-cos/src/fill.rs`, `crates/tinker-pdf/tests/free_text.rs` (new), `docs/features/editing.md` | `free_text.rs` (new) `free_text_in_every_font_case_draws`: each case gets an `/AP` whose text renders where its layout puts it and extracts as written, in both builds; failing first on today's bare annotation. | — |
| AN-03.2 | build | Callout lines (`/CL` with `/LE`), quadding (`/Q`) and the typewriter intent. | `crates/tinker-pdf-cos/src/appearance.rs`, `crates/tinker-pdf/tests/free_text.rs` (new) | `callouts_quadding_and_typewriter`: the callout's geometry and line ending, each quadding's text position, and the typewriter's bare look equal what the test computes. | AN-03.1 |

**Closes the row:** AN-03.1 gives a composite-font free text an `/AP` rendering where its layout puts it and AN-03.2 completes callouts, quadding and the typewriter intent, which is the row's exit.

## AN-04 — Border effects and styles

**Border effects and styles**: cloudy `/BE` with intensity, beveled, inset and underline `/BS`, every dash pattern, a `/Border`'s corner radii

**Today.** a cloudy `/BE`, the beveled, inset and underline `/BS` styles and a `/Border`'s corner radii are drawn solid and square, each a row of the feature doc's refusal table (`features/editing.md:835`)

**Exit criterion.** the cloud's arc geometry and the bevel shading computed in the test; reviewed goldens for the invented looks (H-66); or the owner's ruling that they are refused

**Doc:** — · **Depends on:** H-12

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-04.1 | build | Cloudy borders: `/BE /S /C` with its intensity drawn as arcs along the rectangle or polygon. *Note:* If H-12 rules the cloudy look refused, this step records that instead and the row's refusal stays. | `crates/tinker-pdf-cos/src/appearance.rs`, `crates/tinker-pdf/tests/annotation_borders.rs` (new) | `annotation_borders.rs` (new) `cloud_arcs_follow_their_geometry`: the arcs' centres and radii equal those the test computes from the intensity; a golden family awaiting H-66. | H-12, K-08 |
| AN-04.2 | build | Beveled, inset and underline `/BS` styles, every dash pattern, and a `/Border`'s corner radii. | `crates/tinker-pdf-cos/src/appearance.rs`, `crates/tinker-pdf/tests/annotation_borders.rs` (new), `docs/features/editing.md` | `border_styles_draw_their_geometry`: the bevel's shading, the inset's, the underline, each dash and the rounded corners equal what the test computes; goldens awaiting H-66; the refusal rows go. | AN-04.1 |

**Closes the row:** AN-04.1 and AN-04.2 draw every border with geometry and shading computed in the test as reviewed goldens, or record the owner's refusal, which is the row's exit.

## AN-05 — Line captions and leaders; a caret's paragraph symbol

**Line captions and leaders; a caret's paragraph symbol**

**Today.** a line's caption (`/Cap`) is not drawn and a caret's paragraph symbol (`/Sy /P`) is not drawn, each a row of the feature doc's refusal table (`features/editing.md:832-833`)

**Exit criterion.** caption placement computed from `/CP` and `/CO` in the test; leader geometry from `/LL`, `/LLE`, `/LLO`; the ¶ drawn, or the owner's ruling that it is refused

**Doc:** — · **Depends on:** H-12

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-05.1 | build | Line captions (`/Cap`, placed by `/CP` and `/CO`) and leaders (`/LL`, `/LLE`, `/LLO`) drawn. | `crates/tinker-pdf-cos/src/appearance.rs`, `crates/tinker-pdf/tests/line_captions.rs` (new), `docs/features/editing.md` | `line_captions.rs` (new) `captions_and_leaders_place_as_computed`: the caption's position and the leaders' geometry equal what the test computes from the entries; failing first on today's undrawn caption. | — |
| AN-05.2 | build | A caret's paragraph symbol (`/Sy /P`) drawn. | `crates/tinker-pdf-cos/src/appearance.rs`, `crates/tinker-pdf/tests/line_captions.rs` (new) | `the_carets_paragraph_symbol_is_drawn`: the ¶ is drawn in the caret's box as a golden awaiting H-66, or, if H-12 refuses it, the refusal is recorded. | H-12, K-08 |

**Closes the row:** AN-05.1 places captions and leaders as computed and AN-05.2 draws the ¶ or records its refusal, which is the row's exit.

## AN-06 — Annotation editing, complete

**Annotation editing, complete**: create every markup subtype with every property; vertices, ink strokes and quad points added and cleared; line endings; intents; popups and open state; reply (`/IRT`, `/RT`) set and replies deleted; rotation, optional content and blend; an update cycle that regenerates stale appearances; appearances set from a display list; rollover and down states; synthesised appearances kept out of the file until saved; links updated and deleted

**Today.** `add_annotation`; constructors `highlight`, `square`, `text_note`, `link`; `flatten_annotations`

**Exit criterion.** each property written, re-read and drawn; the update cycle regenerates exactly the annotations whose dictionaries changed; `tpdf annots list` and `remove` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-06.1 | build | Every markup subtype created with every property, by typed constructors over `AnnotationPayload` beside today's four in `annot.rs`. | `crates/tinker-pdf-cos/src/edit/annot.rs`, `crates/tinker-pdf/src/annotations.rs`, `crates/tinker-pdf/tests/annotation_editing.rs` (new), `docs/features/editing.md` | `annotation_editing.rs` (new) `every_subtype_writes_reads_and_draws`: each property written re-reads through the payload and draws as written. | — |
| AN-06.2 | build | Geometry and look edited: vertices, ink strokes and quad points added and cleared, line endings, intents, rotation, optional content and blend. | `crates/tinker-pdf-cos/src/edit/annot.rs`, `crates/tinker-pdf/tests/annotation_editing.rs` (new) | `geometry_and_looks_edit`: each edit re-reads as written and draws as the edited dictionary says. | AN-06.1 |
| AN-06.3 | build | Popups and open state, replies set (`/IRT`, `/RT`) and deleted with their thread, and links updated and deleted. | `crates/tinker-pdf-cos/src/edit/annotations.rs`, `crates/tinker-pdf/tests/annotation_editing.rs` (new) | `popups_replies_and_links_edit`: each edit re-reads as written; deleting a parent removes its replies and popup. | AN-06.1 |
| AN-06.4 | build | The update cycle: changed dictionaries tracked so exactly the stale appearances are regenerated, appearances set from a display list, rollover and down states, and synthesised appearances kept out of the file until saved. | `crates/tinker-pdf-cos/src/edit/annotations.rs`, `crates/tinker-pdf-cos/src/appearance.rs`, `crates/tinker-pdf/tests/annotation_editing.rs` (new) | `the_update_cycle_regenerates_exactly_the_changed`: after edits to some annotations only theirs are regenerated (counted); an unsaved document's file bytes carry none of the synthesised streams. | AN-06.2, AN-06.3 |
| AN-06.5 | build | `tpdf annots list` and `remove`, and a fingerprint of one page of edited annotations. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs`, `crates/tinker-pdf/tests/determinism.rs` | Both subcommands run over `testdata/` fixtures to the facade's output; the fingerprint holds on every K-07 target. | AN-06.4, K-07 |

**Closes the row:** AN-06.1 to AN-06.3 write, re-read and draw each property, AN-06.4 regenerates exactly the changed appearances, and AN-06.5 runs `tpdf annots list` and `remove`, which is the row's exit.

## AN-07 — The review model

**The review model**: reply threads, per-user review states (accepted, rejected, cancelled, completed; marked, unmarked), filtering and sorting, a comment-summary document, migrating comments between versions

**Today.** payloads read; no semantics

**Exit criterion.** fixtures with known threads and states round-trip; the summary lists every comment once

**Doc:** — · **Depends on:** AN-06

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-07.1 | build | Reply threads (`/IRT` trees, `/RT` reply and group) and per-user review states (the `Review` and `Marked` models of `/State`) read and set. | `crates/tinker-pdf/src/review.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/review.rs` (new), `docs/features/editing.md` | `review.rs` (new) `threads_and_states_round_trip`: fixtures with known threads and states round-trip, each user's latest state the one reported. | AN-06 |
| AN-07.2 | build | Filtering and sorting comments by author, date, subtype, state and page. | `crates/tinker-pdf/src/review.rs` (new), `crates/tinker-pdf/tests/review.rs` (new) | `comments_filter_and_sort`: a table of filters and orders gives the lists the test expects. | AN-07.1 |
| AN-07.3 | build | A comment-summary document listing every comment with its page, author, state and replies, linked to its place. | `crates/tinker-pdf/src/review.rs` (new), `crates/tinker-pdf/tests/review.rs` (new) | `the_summary_lists_every_comment_once`: every comment appears once and its link lands on its page. | AN-07.2, ED-45 |
| AN-07.4 | build | Comments migrated between versions: each re-anchored to its text in the new version through ED-22's diff. | `crates/tinker-pdf/src/review.rs` (new), `crates/tinker-pdf/tests/review.rs` (new) | `comments_follow_their_text`: generated version pairs with known moves place each comment on its moved text; a comment whose text was deleted is reported, not placed. | AN-07.1, ED-22 |

**Closes the row:** AN-07.1 round-trips known threads and states and AN-07.3's summary lists every comment once, which is the row's exit; AN-07.2 and AN-07.4 add filtering and migration.

## AN-08 — Annotation exchange through XFDF and FDF

**Annotation exchange through XFDF and FDF**: every markup subtype with appearance data, popups, replies, states, rich text, ink, attachments, page mapping, and delta export

**Today.** XFDF and FDF annotations are `FormDataWarning::NotRead` (`features/forms.md:556`)

**Exit criterion.** export then import reproduces every annotation dictionary; documents exchanged with a generated set match; `tpdf annots export` and `import` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** H-102 (ISO 19444-1)

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-08.1 | build | XFDF export of every markup subtype: appearance data, popups, replies, states, rich text, ink and attachments, through the XML crate's writer. | `crates/tinker-pdf/src/form_data.rs`, `crates/tinker-pdf/tests/annotation_exchange.rs` (new), `docs/features/forms.md` | `annotation_exchange.rs` (new) `xfdf_export_holds_every_entry`: each subtype's export, read by the test as XML, holds every entry ISO 19444-1 names for it. | AN-06, H-102 |
| AN-08.2 | build | XFDF import, retiring `FormDataWarning::NotRead` for annotations. | `crates/tinker-pdf/src/form_data.rs`, `crates/tinker-pdf/tests/annotation_exchange.rs` (new) | `export_then_import_reproduces_every_dictionary`: for each subtype, export then import reproduces its annotation dictionary. | AN-08.1 |
| AN-08.3 | build | FDF annotations exported and imported. | `crates/tinker-pdf/src/form_data.rs`, `crates/tinker-pdf/tests/annotation_exchange.rs` (new) | `fdf_round_trips_every_dictionary`: export then import reproduces every annotation dictionary. | AN-08.2 |
| AN-08.4 | build | Page mapping on import and delta export of what changed since a base revision. | `crates/tinker-pdf/src/form_data.rs`, `crates/tinker-pdf/tests/annotation_exchange.rs` (new) | `mapping_and_deltas`: a generated set imported with a page map lands on the mapped pages; a delta holds exactly the changed annotations. | AN-08.3 |
| AN-08.5 | build | `tpdf annots export` and `import`. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | CLI tests exchange a generated set between two documents and the annotations match. | AN-08.4 |

**Closes the row:** AN-08.2 and AN-08.3 reproduce every annotation dictionary through export and import, AN-08.4 matches exchanged sets, and AN-08.5 runs the CLI, which is the row's exit.

## AN-09 — Measurement

**Measurement**: `/Measure` rectilinear dictionaries (scale ratio and number formats), viewports `/VP`, dimension intents with captions; distances, areas and perimeters computed

**Today.** line captions refused; `/Measure` not read

**Exit criterion.** a drawing with a known scale yields the known lengths and areas

**Doc:** — · **Depends on:** AN-05

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-09.1 | build | `/Measure` rectilinear dictionaries (scale ratio and number formats) and viewports (`/VP`) read and written. | `crates/tinker-pdf/src/measure.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/measurement.rs` (new) | `measurement.rs` (new) `measure_dictionaries_round_trip`: every entry written reads back as written. | — |
| AN-09.2 | build | Distances, areas and perimeters of line, polyline, polygon and square annotations through the viewport's measure and its number-format chain. | `crates/tinker-pdf/src/measure.rs` (new), `crates/tinker-pdf/tests/measurement.rs` (new) | `a_known_scale_gives_known_lengths_and_areas`: a drawing at a known scale yields the known lengths, areas and perimeters, formatted as its number formats say. | AN-09.1 |
| AN-09.3 | build | Dimension intents with captions: each dimension annotation's caption set to its formatted measurement. | `crates/tinker-pdf/src/measure.rs` (new), `crates/tinker-pdf/tests/measurement.rs` (new), `docs/features/editing.md` | `dimension_captions_show_their_measurement`: each intent's caption equals the formatted value the test computes and is drawn by AN-05's captions. | AN-09.2, AN-05 |

**Closes the row:** AN-09.2 yields the known lengths and areas of a drawing at a known scale, which is the row's exit; AN-09.1 and AN-09.3 read the dictionaries and caption the dimensions.

## AN-10a — Geospatial data read and written

**Geospatial data read and written**: `/Measure /GEO`, `/GCS`, `/GPTS`, `/LPTS`, `/PCSM`, `/DCS`, `/PDU`, and 2.0's `/PtData`

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -w 'GPTS|LPTS|PtData|GCS' crates` finds nothing

**Exit criterion.** fixtures round-trip every key; a page point maps to the registered point list by the dictionary's affine

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-10a.1 | build | Geospatial dictionaries read and written: `/Measure /GEO` with `/GCS`, `/GPTS`, `/LPTS`, `/PCSM`, `/DCS`, `/PDU`, and 2.0's `/PtData`. | `crates/tinker-pdf/src/geo.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/geospatial.rs` (new), `docs/features/editing.md` | `geospatial.rs` (new) `geo_dictionaries_round_trip`: fixtures round-trip every key. | — |
| AN-10a.2 | build | A page point mapped to the registered point list by the dictionary's affine between the viewport, `/LPTS` and `/GPTS`. | `crates/tinker-pdf/src/geo.rs` (new), `crates/tinker-pdf/tests/geospatial.rs` (new) | `page_points_map_by_the_affine`: page points map to the registered points the test computes from the dictionary's affine. | AN-10a.1 |

**Closes the row:** AN-10a.1 round-trips every key and AN-10a.2 maps page points by the affine, which is the row's exit.

## AN-10b — Geospatial projection arithmetic

**Geospatial projection arithmetic**: page point to latitude and longitude through the stated coordinate system

**Today.** absent — the owner's parity decision of 9 October 2026; latitude and longitude appear only as XMP property names the archival validator types (`crates/tinker-pdf/src/pdfa/xmp_schemas.rs`)

**Exit criterion.** published projection examples (the projection standards' worked numbers) reproduced in the test

**Doc:** `design/geospatial.md` (owed) · **Depends on:** AN-10a

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-10b.1 | design | Create `docs/design/geospatial.md`: the coordinate systems resolved (from the WKT a document carries; whether a code table is vendored is a licence question, H-116), datums and their transformations, the projections built and their published worked numbers, the arithmetic, and what is refused by name. | `docs/design/geospatial.md` (new) | The doc exists in the design-doc order, names AN-10b.2 to AN-10b.7's exit tests and their published examples, and is reviewed. | AN-10a |
| AN-10b.2 | build | Coordinate-system text parsed: geographic and projected definitions in well-known text, with their datum, ellipsoid, units and projection parameters. | `crates/tinker-pdf-math/src/geo/wkt.rs` (new), `crates/tinker-pdf-math/src/lib.rs` | Unit tests: published definitions parse to their parameters; malformed text is a typed error. | AN-10b.1 |
| AN-10b.3 | build | Ellipsoids and datums: geodetic to geocentric and back, and the seven-parameter transformation a definition states. | `crates/tinker-pdf-math/src/geo/datum.rs` (new) | Unit tests: the published worked numbers for each conversion are reproduced to their stated precision. | AN-10b.2 |
| AN-10b.4 | build | Transverse Mercator forward and inverse. | `crates/tinker-pdf-math/src/geo/projections.rs` (new) | Unit tests: the published worked numbers are reproduced both ways. | AN-10b.3 |
| AN-10b.5 | build | Lambert conformal conic (one and two standard parallels) and Albers equal-area. | `crates/tinker-pdf-math/src/geo/projections.rs` (new) | Unit tests: the published worked numbers are reproduced both ways. | AN-10b.4 |
| AN-10b.6 | build | Mercator, polar stereographic and the rest the design lists. | `crates/tinker-pdf-math/src/geo/projections.rs` (new) | Unit tests: the published worked numbers are reproduced both ways for each projection. | AN-10b.4 |
| AN-10b.7 | build | Page point to latitude and longitude and back, through AN-10a's registration and the stated system's projection. | `crates/tinker-pdf/src/geo.rs` (new), `crates/tinker-pdf/tests/geospatial.rs` (new), `docs/features/editing.md` | `page_points_give_latitude_and_longitude`: generated maps with known registrations give the known coordinates for known page points, and back. | AN-10b.5, AN-10b.6 |

**Closes the row:** AN-10b.3 to AN-10b.6 reproduce the projection standards' worked numbers in the test, and AN-10b.7 maps page points through them, which is the row's exit.

## AN-11a — 3D assets

**3D assets**: U3D (ECMA-363) and PRC (ISO 14739-1) parsed, views (`/3DV`) read, models exported as meshes

**Today.** the `/3DD` payload is read by reference only (AN-01)

**Exit criterion.** the format specifications' sample structures parsed; exported meshes' vertex and face counts equal the stated ones

**Doc:** `design/3d.md` (owed) · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-11a.1 | design | Create `docs/design/3d.md` for AN-11a, AN-11b and AN-11c: U3D and PRC parsing, the scene model (nodes, meshes, materials, lights, views), the software rasteriser, the scripting object model, and their milestones. | `docs/design/3d.md` (new) | The doc exists in the design-doc order, names this row's and AN-11b's and AN-11c's exit tests, and is reviewed. | — |
| AN-11a.2 | build | U3D blocks: the file header, block structure, declarations and modifier chains read into the scene model. | `crates/tinker-pdf-3d/src/u3d.rs` (new), `crates/tinker-pdf-3d/src/lib.rs` (new), `crates/tinker-pdf-3d/Cargo.toml` (new), `Cargo.toml`, `docs/architecture.md` | Unit tests: ECMA-363's sample structures parse to the declarations they state; the new leaf crate is in `docs/architecture.md`. | AN-11a.1 |
| AN-11a.3 | build | U3D's continuous-level-of-detail meshes: the base mesh decompressed and resolution updates applied. | `crates/tinker-pdf-3d/src/u3d.rs` (new), `crates/tinker-pdf-3d/src/clod.rs` (new) | Unit tests: sample meshes decompress to their stated vertex and face counts at every resolution. | AN-11a.2 |
| AN-11a.4 | build | PRC: the file structure, compressed sections through the in-tree inflate, and tessellated entities into meshes. | `crates/tinker-pdf-3d/src/prc.rs` (new) | Unit tests: ISO 14739-1's sample structures parse; their meshes' vertex and face counts equal the stated ones. | AN-11a.2, H-102 |
| AN-11a.5 | build | `/3DV` views read (camera, projection, render mode, lighting, cross-sections, node states), and models exported as meshes. | `crates/tinker-pdf/src/three_d.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/three_d.rs` (new), `docs/features/editing.md` | `three_d.rs` (new) `views_read_and_meshes_export`: views read as written; exported meshes' vertex and face counts equal the stated ones. | AN-11a.3, AN-11a.4 |

**Closes the row:** AN-11a.2 to AN-11a.4 parse the specifications' sample structures and AN-11a.5 exports meshes with the stated counts, which is the row's exit.

## AN-11b — 3D views drawn into the page

**3D views drawn into the page** by a software rasteriser (camera, render mode, lighting, cross-sections, node visibility)

**Today.** absent — the owner's parity decision of 9 October 2026; a `/3D` stream is carried by reference (AN-01) and nothing parses U3D or PRC: `rg -i 'u3d' crates` finds fixtures and the archival rule that names the subtype

**Exit criterion.** analytic scenes (a cube under a stated camera) render to the projections computed in the test; determinism on four targets

**Doc:** `design/3d.md` (owed) · **Depends on:** AN-11a

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-11b.1 | design | Add the rasteriser's milestones to `docs/design/3d.md`: deterministic fixed-point or exactly specified float arithmetic, clipping, depth buffering, render modes, lighting schemes, cross-sections and node visibility. | `docs/design/3d.md` | The section names AN-11b.2 to AN-11b.6's exit tests and is reviewed. | AN-11a.1 |
| AN-11b.2 | build | The pipeline: a view's camera, vertex transform, clipping and triangle rasterisation with a depth buffer. | `crates/tinker-pdf-3d/src/raster.rs` (new) | Unit tests: an analytic cube under a stated camera renders to the projection computed in the test. | AN-11b.1, AN-11a |
| AN-11b.3 | build | Render modes: solid, wireframe, transparent, illustration and the rest 13.6.6 names. | `crates/tinker-pdf-3d/src/raster.rs` (new) | Unit tests: each mode's analytic scene renders as the test computes. | AN-11b.2 |
| AN-11b.4 | build | Lighting schemes as 13.6.6 lists them. | `crates/tinker-pdf-3d/src/lighting.rs` (new) | Unit tests: each scheme's shading of an analytic scene equals the test's computation. | AN-11b.2 |
| AN-11b.5 | build | Cross-sections and node visibility. | `crates/tinker-pdf-3d/src/raster.rs` (new) | Unit tests: a cut cube and a hidden node render as computed. | AN-11b.3 |
| AN-11b.6 | build | A 3D annotation's view drawn into its rectangle when the page renders, and a determinism fingerprint of one. | `crates/tinker-pdf/src/three_d.rs` (new), `crates/tinker-pdf/tests/three_d.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | `views_draw_into_the_page`: the page's pixels in the rectangle equal the scene's render; the fingerprint holds on every K-07 target. | AN-11b.4, AN-11b.5, K-07 |

**Closes the row:** AN-11b.2 to AN-11b.5 render analytic scenes to their computed projections and AN-11b.6 draws them into the page with a fingerprint on four targets, which is the row's exit.

## AN-11c — The 3D scripting API

**The 3D scripting API**

**Today.** absent — the owner's parity decision of 9 October 2026; no 3D content is parsed (AN-11b's search)

**Exit criterion.** the object model's stated behaviours held to expectations a person wrote (H-70)

**Doc:** `design/3d.md` (owed) · **Depends on:** AN-11b, FJ-11, H-70

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-11c.1 | design | Add the 3D scripting object model's milestones to `docs/design/3d.md`: its members as published, their binding through FJ-11's host objects, events and the render loop. | `docs/design/3d.md` | The section names AN-11c.2 to AN-11c.4's exit tests and is reviewed. | AN-11a.1 |
| AN-11c.2 | build | The scene, node, camera and material objects bound through FJ-11's host objects. | `crates/tinker-pdf/src/three_d_script.rs` (new), `crates/tinker-pdf/tests/three_d.rs` (new) | `the_3d_object_model_behaves_as_stated`: each member's behaviour holds to the expectations a person wrote under H-70. | AN-11c.1, AN-11b, FJ-11, H-70 |
| AN-11c.3 | build | Events and the render loop: mouse, key and per-frame events driving AN-11b's renderer. | `crates/tinker-pdf/src/three_d_script.rs` (new), `crates/tinker-pdf/tests/three_d.rs` (new) | `events_drive_the_render`: scripted events change the drawn view as the expectations state. | AN-11c.2 |
| AN-11c.4 | test | A coverage list of the object model's members: each implemented and held to an expectation, or throwing a typed refusal naming itself. | `crates/tinker-pdf/tests/three_d.rs` (new) | The coverage test counts every listed member, and none is `undefined`. | AN-11c.3 |

**Closes the row:** AN-11c.2 and AN-11c.3 hold the object model's stated behaviours to the expectations a person wrote, which is the row's exit; AN-11c.4 counts its coverage.

## AN-12 — The multimedia model

**The multimedia model**: Screen annotations, rendition actions, media clips with permissions, Sound annotations decoded (raw, signed, µ-law, A-law) to PCM, Movie, 2.0 RichMedia assets and configurations — extracted and exposed for a host to play

**Today.** payload by reference; no appearance synthesis for these subtypes

**Exit criterion.** a sound's samples decoded by the codec formulas in the test; assets extracted byte-equal

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-12.1 | build | Screen annotations, rendition actions and media clips with their permissions parsed into a typed model for a host. | `crates/tinker-pdf/src/media.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/media.rs` (new), `docs/features/editing.md` | `media.rs` (new) `renditions_and_clips_parse`: fixtures parse to the model; clip data extracts byte-equal. | — |
| AN-12.2 | build | Sound annotations and objects decoded to PCM from raw, signed, µ-law and A-law samples at their rate, size and channel count. | `crates/tinker-pdf/src/media.rs` (new), `crates/tinker-pdf/tests/media.rs` (new) | `sounds_decode_by_the_codec_formulas`: each encoding's samples equal the codec formulas computed in the test. | AN-12.1 |
| AN-12.3 | build | Movie annotations and 2.0 RichMedia assets, configurations and instances extracted. | `crates/tinker-pdf/src/media.rs` (new), `crates/tinker-pdf/tests/media.rs` (new) | `movies_and_rich_media_extract`: every asset extracts byte-equal; configurations read as written. | AN-12.1 |
| AN-12.4 | build | `tpdf media list` and `extract`. | `tools/tpdf/src/main.rs` | CLI tests list and extract every media fixture's assets byte-equal. | AN-12.2, AN-12.3 |

**Closes the row:** AN-12.2 decodes sounds by the codec formulas and AN-12.1 and AN-12.3 extract assets byte-equal, which is the row's exit.

## AN-13 — Flattening policies

**Flattening policies**: all, print-visible or screen-visible annotations; widgets separately; signatures kept

**Today.** `flatten_annotations`

**Exit criterion.** each policy's survivors match the flags; the render under each usage is unchanged; `tpdf annots flatten`, `tpdf forms flatten` and `tpdf bake` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** RC-13

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-13.1 | build | Flattening policies: all, print-visible or screen-visible annotations by their flags and RC-13's usage, widgets separately, and signatures kept. | `crates/tinker-pdf-cos/src/edit/annotations.rs`, `crates/tinker-pdf/tests/flatten_policies.rs` (new), `docs/features/editing.md` | `flatten_policies.rs` (new) `each_policy_keeps_its_survivors`: each policy's survivors match the flags, and the render under each usage is unchanged. | RC-13 |
| AN-13.2 | build | `tpdf annots flatten`, `tpdf forms flatten` and `tpdf bake`. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | Each command runs over `testdata/` fixtures to the facade's output. | AN-13.1 |

**Closes the row:** AN-13.1 keeps each policy's survivors with the render unchanged and AN-13.2 runs the three commands, which is the row's exit.

## AN-14 — Every field type created

**Every field type created**: push buttons with captions, icons and layout; list boxes; empty signature fields; file-select, password, rich-text and comb fields; appearance characteristics (`/MK`) colours; unique names generated from a prefix

**Today.** text, checkbox, radio and choice fields created (`features/forms.md:16-191`)

**Exit criterion.** each type created, filled through the editor and re-read; appearances drawn and validated

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-14.1 | build | Push buttons with captions, icons and layout (`/MK /CA`, `/I`, `/TP`), and `/MK`'s border and background colours for every field. | `crates/tinker-pdf-cos/src/edit/forms.rs`, `crates/tinker-pdf-cos/src/appearance.rs`, `crates/tinker-pdf/tests/field_types.rs` (new), `docs/features/forms.md` | `field_types.rs` (new) `push_buttons_create_and_draw`: each layout is created, re-read and drawn, and validates strictly. | — |
| AN-14.2 | build | List boxes, single and multiple selection, with export values and top index. | `crates/tinker-pdf-cos/src/edit/forms.rs`, `crates/tinker-pdf/tests/field_types.rs` (new) | `list_boxes_create_fill_and_draw`: created, filled through the editor, re-read and drawn. | AN-14.1 |
| AN-14.3 | build | Empty signature fields, and file-select, password, rich-text and comb text fields. | `crates/tinker-pdf-cos/src/edit/forms.rs`, `crates/tinker-pdf-cos/src/fill.rs`, `crates/tinker-pdf/tests/field_types.rs` (new) | `special_text_fields_create_fill_and_draw`: each is created, filled, re-read and drawn (comb cells as computed), and validates strictly. | AN-14.1 |
| AN-14.4 | build | Unique names generated from a prefix across the field hierarchy, and a fingerprint of one page of every field type. | `crates/tinker-pdf-cos/src/edit/forms.rs`, `crates/tinker-pdf/tests/field_types.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | `names_are_unique`: generated names never collide with existing ones; the fingerprint holds on every K-07 target. | AN-14.2, AN-14.3, K-07 |

**Closes the row:** AN-14.1 to AN-14.3 create, fill, re-read and draw every field type, validated, which is the row's exit.

## AN-15 — An interactive form-filling session

**An interactive form-filling session**: focus and tab order (`/Tabs`), key and character input routed through keystroke events, caret and selection, input-method-safe replacement, per-field undo and redo, editable combo boxes, multi-select lists, button toggling (radios in unison, no toggle to off), password masking, file selection, comb and multi-line scrolling, highlight colour, hit testing, invalidated rectangles, text direction per field

**Today.** fill by API only (`fill_field`, `keystroke`)

**Exit criterion.** a scripted session over the committed form fixtures produces the field values and appearances a direct fill produces; invalidated rectangles cover exactly the changed pixels

**Doc:** `design/form-session.md` (owed) · **Depends on:** FJ-06

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-15.1 | design | Create `docs/design/form-session.md`: the session's state and host interface, focus and tab order, input routed through FJ-06's keystroke events, caret, selection and composition, undo, each field type's interaction, invalidation, and per-field text direction. | `docs/design/form-session.md` (new) | The doc exists in the design-doc order, names AN-15.2 to AN-15.8's exit tests, and is reviewed. | FJ-06 |
| AN-15.2 | build | Focus, tab order by `/Tabs` and hit testing of widgets. | `crates/tinker-pdf/src/form_session.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/form_session.rs` (new) | `form_session.rs` (new) `tab_order_and_hit_tests`: generated forms tab in the order each `/Tabs` value states; points hit the widget under them. | AN-15.1 |
| AN-15.3 | build | Key and character input through keystroke events, caret and selection, and composition text replacing the selection in one step. | `crates/tinker-pdf/src/form_session.rs` (new), `crates/tinker-pdf/tests/form_session.rs` (new) | `typed_input_fills_as_a_direct_fill`: typing into each text field yields the value and appearance a direct fill gives; events fire in the stated order. | AN-15.2 |
| AN-15.4 | build | Per-field undo and redo, editable combo boxes and multi-select lists. | `crates/tinker-pdf/src/form_session.rs` (new), `crates/tinker-pdf/tests/form_session.rs` (new) | `undo_combos_and_lists`: undo and redo restore each step's value; combos and lists end with the direct-fill values. | AN-15.3 |
| AN-15.5 | build | Buttons: radios toggling in unison, no toggle to off, check boxes, password masking and file selection through the host. | `crates/tinker-pdf/src/form_session.rs` (new), `crates/tinker-pdf/tests/form_session.rs` (new) | `buttons_passwords_and_files`: each ends with the direct-fill value and appearance; a password field never draws its characters. | AN-15.3 |
| AN-15.6 | build | Comb and multi-line scrolling, the highlight mode on activation, and text direction per field. | `crates/tinker-pdf/src/form_session.rs` (new), `crates/tinker-pdf/tests/form_session.rs` (new) | `scrolling_highlight_and_direction`: long text scrolls to keep the caret visible, highlight draws per `/H`, and right-to-left fields edit in logical order. | AN-15.4 |
| AN-15.7 | build | Invalidated rectangles reported after each change. | `crates/tinker-pdf/src/form_session.rs` (new), `crates/tinker-pdf/tests/form_session.rs` (new) | `invalidated_rectangles_cover_exactly_the_changed_pixels`: for each change, the reported rectangles cover the pixels a render before and after differ in, and no other. | AN-15.6 |
| AN-15.8 | test | A scripted session over every committed form fixture, a fuzz target of random sessions, and a fingerprint of one session's result. | `crates/tinker-pdf/tests/form_session.rs` (new), `fuzz/fuzz_targets/form_session.rs` (new), `fuzz/Cargo.toml`, `docs/verification.md`, `crates/tinker-pdf/tests/determinism.rs` | `a_scripted_session_equals_a_direct_fill`: every committed form ends with the values and appearances a direct fill produces; the fuzz target runs clean with the roster regenerated; the fingerprint holds on every K-07 target. | AN-15.5, AN-15.7, K-07 |

**Closes the row:** AN-15.8 produces a direct fill's values and appearances by a scripted session and AN-15.7's rectangles cover exactly the changed pixels, which is the row's exit.

## AN-16 — Rich text in free text and fields

**Rich text in free text and fields**: `/RC`, `/RV` and `/DS` laid out by the CSS and layout engine

**Today.** `/RC` rich text and `/DS` default style are drawn as plain `/Contents` (`features/editing.md:830`)

**Exit criterion.** a free text annotation with `/RC` and `/DS` given an `/AP` whose text renders where its layout puts it; each rich-text fixture renders as the same XHTML laid out by the EPUB path at that box

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-16.1 | build | Rich text parsed: `/RC`'s XHTML body and `/DS`'s default style read into the layout crate's tree and the CSS crate's styles, by 12.7.3.4's subset. | `crates/tinker-pdf/src/rich_text.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/rich_text.rs` (new) | `rich_text.rs` (new) `rich_text_parses_to_its_tree`: fixtures parse to the elements and computed styles the test expects; an unsupported property is a typed warning. | — |
| AN-16.2 | build | Free text given an `/AP` from `/RC` and `/DS`, laid out by the layout engine in the annotation's box. | `crates/tinker-pdf/src/rich_text.rs` (new), `crates/tinker-pdf-cos/src/appearance.rs`, `crates/tinker-pdf/tests/rich_text.rs` (new), `docs/features/editing.md` | `rich_free_text_renders_as_its_xhtml`: each fixture renders as the same XHTML laid out by the EPUB path at that box, and extracts as written, in both builds. | AN-16.1 |
| AN-16.3 | build | Rich-text fields: a field with the rich-text flag drawn from `/RV` and `/DS` the same way. | `crates/tinker-pdf/src/rich_text.rs` (new), `crates/tinker-pdf-cos/src/fill.rs`, `crates/tinker-pdf/tests/rich_text.rs` (new), `docs/features/forms.md` | `rich_fields_render_as_their_xhtml`: each fixture renders as the same XHTML laid out at the widget's box. | AN-16.2 |

**Closes the row:** AN-16.2 and AN-16.3 give rich text an `/AP` rendering as the same XHTML laid out by the EPUB path, which is the row's exit.

## AN-17 — Signature fields through the form layer

**Signature fields through the form layer**: create one empty, sign it (through SG rows), clear it

**Today.** refused: "Filling or signing a signature field" (`features/forms.md:572`)

**Exit criterion.** a field created, signed and verified through the form layer gives the verdict the signing path gives

**Doc:** — · **Depends on:** SG-06

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-17.1 | build | A signature field signed from the form layer: an empty field found by name and signed through SG-06's path, its widget given the signature's appearance. | `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/signature_fields.rs` (new), `docs/features/forms.md` | `signature_fields.rs` (new) `a_field_signed_through_the_form_layer_verifies`: a field created, signed and verified through the form layer gives the verdict the signing path gives. | SG-06, AN-14 |
| AN-17.2 | build | Clearing a signature field: the value removed in an incremental save and the field left signable again, retiring the refusal in the forms doc. | `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/signature_fields.rs` (new), `docs/features/forms.md` | `a_cleared_field_signs_again`: the cleared field carries no value, earlier signatures still verify, and it signs again. | AN-17.1 |

**Closes the row:** AN-17.1 gives the signing path's verdict through the form layer and AN-17.2 clears and re-signs, which is the row's exit.

## AN-18 — Page templates

**Page templates** (12.7.6): the `/Templates` tree, spawning with field renaming, hidden templates

**Today.** absent — the owner's parity decision of 9 October 2026; `/Templates` is named only in a comment of the FDF reader (`crates/tinker-pdf/src/form_data.rs:27`); nothing spawns a page

**Exit criterion.** a spawned page's fields are named by the 12.7.6 scheme and fill independently

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-18.1 | build | Page templates read: the `/Templates` and `/Pages` name trees, hidden and visible templates listed. | `crates/tinker-pdf-cos/src/trees.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/page_templates.rs` (new) | `page_templates.rs` (new) `templates_are_listed`: fixtures list their named templates, hidden ones marked. | — |
| AN-18.2 | build | Spawning: a template instantiated as a new page, its fields renamed by 12.7.6's scheme. | `crates/tinker-pdf-cos/src/edit/page_ops.rs`, `crates/tinker-pdf/tests/page_templates.rs` (new), `docs/features/forms.md` | `spawned_fields_are_named_and_fill_independently`: a template spawned twice gives fields named by the scheme that fill independently. | AN-18.1 |

**Closes the row:** AN-18.2 names a spawned page's fields by 12.7.6's scheme so they fill independently, which is the row's exit.

## AN-19 — Submit, reset and import as payloads

**Submit, reset and import as payloads**: every submit flag (include/exclude, empty fields, HTML-form, FDF, XFDF or PDF format, GET, coordinates, incremental appends, annotations, canonical dates) producing the exact bytes, URL and method for the host to send; reset with field lists; import data

**Today.** FDF and XFDF read, written and applied

**Exit criterion.** each flag combination's payload equals the bytes computed in the test from 12.7.6.2's rules

**Doc:** — · **Depends on:** AN-20

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-19.1 | build | Submit payloads: field selection (include and exclude lists, empty fields) in HTML-form, FDF, XFDF or PDF format, by GET or POST, handed to the host as bytes, URL and method. | `crates/tinker-pdf/src/form_actions.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/submit.rs` (new), `docs/features/forms.md` | `submit.rs` (new) `payloads_equal_their_computation`: each combination's bytes, URL and method equal those computed in the test from 12.7.6.2's rules. | AN-20 |
| AN-19.2 | build | The rest of the submit flags: coordinates, incremental appends, annotations and canonical dates. | `crates/tinker-pdf/src/form_actions.rs` (new), `crates/tinker-pdf/tests/submit.rs` (new) | `every_flag_shapes_the_payload`: each flag's payload equals the test's computation. | AN-19.1 |
| AN-19.3 | build | Reset with field lists, and import data applied from an FDF the host supplies. | `crates/tinker-pdf/src/form_actions.rs` (new), `crates/tinker-pdf/tests/submit.rs` (new) | `reset_and_import`: values after reset and import equal the test's computation. | AN-19.1 |

**Closes the row:** AN-19.1 and AN-19.2 make every flag combination's payload equal the bytes computed from 12.7.6.2's rules, which is the row's exit; AN-19.3 adds reset and import.

## AN-20 — Every action typed

**Every action typed**: `/JavaScript`, `/SubmitForm`, `/ResetForm`, `/ImportData`, `/Hide`, `/SetOCGState`, `/Rendition`, `/Trans`, `/GoTo3DView`, `/GoToE`, `/GoToDp`, `/Thread`, `/Sound`, `/Movie`, `/RichMediaExecute`; chains through `/Next` with cycle detection; each handed to the host as a typed request, the engine executing none that needs the outside world

**Today.** `Action` has `GoTo`, `GoToR`, `Uri`, `Named`, `Launch` and `Other` (`crates/tinker-pdf-cos/src/dest.rs:88-114`; `features/document-model.md:433`)

**Exit criterion.** a fixture per action type read into its arm; `Hide`, `SetOCGState` and `ResetForm` applied by the engine and held to their results

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-20.1 | build | `Action` gains an arm for every type the row lists, each with its entries, beside today's six in `dest.rs`. | `crates/tinker-pdf-cos/src/dest.rs`, `crates/tinker-pdf/tests/actions.rs` (new), `docs/features/document-model.md` | `actions.rs` (new) `every_action_type_reads_into_its_arm`: a fixture per type reads into its arm with its entries. | — |
| AN-20.2 | build | `/Next` chains walked with cycle detection. | `crates/tinker-pdf-cos/src/dest.rs`, `crates/tinker-pdf/tests/actions.rs` (new) | `chains_walk_and_cycles_stop`: chains yield their actions in order; a cycle stops with a typed warning. | AN-20.1 |
| AN-20.3 | build | `Hide`, `SetOCGState` and `ResetForm` applied by the engine; every action needing the outside world handed to the host as a typed request. | `crates/tinker-pdf/src/form_actions.rs` (new), `crates/tinker-pdf/tests/actions.rs` (new) | `engine_actions_apply`: hidden flags, layer states and field values after each action equal the expected results; the others yield their typed requests and change nothing. | AN-20.2 |

**Closes the row:** AN-20.1 reads a fixture per action type into its arm and AN-20.3 applies `Hide`, `SetOCGState` and `ResetForm`, which is the row's exit.

## AN-21 — Batch filling and forms from data

**Batch filling and forms from data**: fill a template from many CSV or JSON rows into many outputs or one; create a form from a JSON description

**Today.** FDF/XFDF apply and `add_field`

**Exit criterion.** N rows produce N documents whose field values equal the rows; `tpdf forms fill` and `reset` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** AN-14

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-21.1 | build | A template filled from CSV or JSON rows into one document per row, or into one document of all copies with fields renamed per copy. | `crates/tinker-pdf/src/form_batch.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/form_batch.rs` (new), `docs/features/forms.md` | `form_batch.rs` (new) `n_rows_make_n_filled_documents`: N rows produce N documents (or N copies) whose field values equal the rows. | AN-14 |
| AN-21.2 | build | A form created from a JSON description of its fields. | `crates/tinker-pdf/src/form_batch.rs` (new), `crates/tinker-pdf/tests/form_batch.rs` (new) | `a_form_from_json_re_reads_to_its_description`: the created form's fields re-read to the description and validate strictly. | AN-21.1 |
| AN-21.3 | build | `tpdf forms fill` and `reset`. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | Both commands run over `testdata/` fixtures to the facade's output. | AN-21.1 |

**Closes the row:** AN-21.1 fills N documents from N rows and AN-21.3 runs `tpdf forms fill` and `reset`, which is the row's exit.

## AN-22 — Form data export and aggregation

**Form data export and aggregation**: every field with type, options and flags to CSV, JSON or XML; many returned copies merged into one table

**Today.** `fields` listing; FDF/XFDF export

**Exit criterion.** export re-imported reproduces the values; aggregation of generated copies equals the generated table; `tpdf forms list` and `export` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-22.1 | build | Every field exported with its type, options and flags to CSV, JSON or XML. | `crates/tinker-pdf/src/form_batch.rs` (new), `crates/tinker-pdf/tests/form_export.rs` (new), `docs/features/forms.md` | `form_export.rs` (new) `exports_re_import_to_the_values`: each format re-imported reproduces every field's value. | — |
| AN-22.2 | build | Many returned copies merged into one table, and `tpdf forms list` and `export`. | `crates/tinker-pdf/src/form_batch.rs` (new), `crates/tinker-pdf/tests/form_export.rs` (new), `tools/tpdf/src/main.rs` | `copies_aggregate_into_the_table`: generated copies aggregate to the generated table; both commands run over `testdata/` fixtures. | AN-22.1 |

**Closes the row:** AN-22.1 re-imports exports to the values and AN-22.2 aggregates copies and runs the CLI, which is the row's exit.

## AN-23 — The rest of FDF and XFDF

**The rest of FDF and XFDF**: page templates, JavaScript, appearances, flags and rich text; encrypted FDF

**Today.** `FormDataWarning::NotRead`; `FormDataError::Encrypted` (`features/forms.md:556-557`)

**Exit criterion.** every key read and applied; an encrypted FDF opened with its password

**Doc:** — · **Depends on:** AN-08

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-23.1 | build | The rest of FDF and XFDF read and applied: page templates, JavaScript, appearances, flags and rich text, retiring `FormDataWarning::NotRead` for each. | `crates/tinker-pdf/src/form_data.rs`, `crates/tinker-pdf/tests/form_data_rest.rs` (new), `docs/features/forms.md` | `form_data_rest.rs` (new) `every_key_reads_and_applies`: fixtures carrying each key apply it, and none warns `NotRead`. | AN-08 |
| AN-23.2 | build | Encrypted FDF opened with its password, retiring `FormDataError::Encrypted`. | `crates/tinker-pdf/src/form_data.rs`, `crates/tinker-pdf/tests/form_data_rest.rs` (new) | `an_encrypted_fdf_opens_with_its_password`: the fixture applies with the right password and refuses with a wrong one. | AN-23.1 |

**Closes the row:** AN-23.1 reads and applies every key and AN-23.2 opens an encrypted FDF, which is the row's exit.

## AN-24 — Barcode fields for paper forms

**Barcode fields for paper forms**: a field drawing a PDF417, QR or Data Matrix encoding of other fields' values, updated by a calculation

**Today.** absent — the owner's parity decision of 9 October 2026; no symbology is encoded (ED-49's search)

**Exit criterion.** the drawn symbol decodes (ED-50) to the values after every recalculation

**Doc:** — · **Depends on:** ED-49, ED-50, FJ-07

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-24.1 | build | A barcode field: a widget whose appearance draws a PDF417, QR or Data Matrix symbol (ED-49) of its value. | `crates/tinker-pdf-cos/src/appearance.rs`, `crates/tinker-pdf/tests/barcode_fields.rs` (new), `docs/features/forms.md` | `barcode_fields.rs` (new) `the_drawn_symbol_decodes_to_the_value`: each symbology's drawn symbol decodes through ED-50 to the field's value. | ED-49, ED-50 |
| AN-24.2 | build | The value computed from other fields by a calculation, and the symbol redrawn after each recalculation. | `crates/tinker-pdf-cos/src/calc.rs`, `crates/tinker-pdf/tests/barcode_fields.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | `the_symbol_follows_every_recalculation`: after each fill of a source field the drawn symbol decodes to the recalculated values; a fingerprint holds on every K-07 target. | AN-24.1, FJ-07, K-07 |

**Closes the row:** AN-24.2 redraws the symbol so it decodes to the values after every recalculation, which is the row's exit.

## AN-25 — Form-field detection on flat forms

**Form-field detection on flat forms**: boxes, underlines, comb cells, checkboxes and their labels turned into named fields

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'detect_fields|field_detection' crates tools` finds nothing

**Exit criterion.** generated flat forms with known fields detected with recall and precision ≥ the floor the design states

**Doc:** `design/form-detection.md` (owed) · **Depends on:** FT-30

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-25.1 | design | Create `docs/design/form-detection.md`: candidates from a flat page's vectors (boxes, underlines, comb cells, checkboxes), labels found by FT-30's reading order, naming, field kinds, and the recall and precision floor with how generated flat forms measure it. | `docs/design/form-detection.md` (new) | The doc exists in the design-doc order, states the floor, names AN-25.2 to AN-25.5's exit tests, and is reviewed. | FT-30 |
| AN-25.2 | build | Candidates found: boxes and underlines from the page's paths, comb runs of equal cells, and checkbox squares. | `crates/tinker-pdf/src/form_detect.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/form_detection.rs` (new) | `form_detection.rs` (new) `candidates_are_found`: generated flat forms' candidates are found with recall and precision at or above the design's floor. | AN-25.1 |
| AN-25.3 | build | Labels associated by reading order and proximity, and names made from them, safe and unique. | `crates/tinker-pdf/src/form_detect.rs` (new), `crates/tinker-pdf/tests/form_detection.rs` (new) | `labels_name_their_fields`: names match the generated labels at the design's floor. | AN-25.2 |
| AN-25.4 | build | Detections turned into fields (text, comb text with its length, check box) through AN-14's creation. | `crates/tinker-pdf/src/form_detect.rs` (new), `crates/tinker-pdf/tests/form_detection.rs` (new) | `detected_fields_meet_the_floor`: generated flat forms with known fields are detected with recall and precision at or above the floor, and every created field fills. | AN-25.3, AN-14 |
| AN-25.5 | build | `tpdf forms detect`, the speed row and a fingerprint of one detected form. | `tools/tpdf/src/main.rs`, `crates/tinker-pdf/benches/engine.rs`, `crates/tinker-pdf/tests/determinism.rs`, `docs/features/forms.md` | The command runs over `testdata/` fixtures to the facade's output; the speed row is in the benchmark set; the fingerprint holds on every K-07 target. | AN-25.4, MS-04, K-07 |

**Closes the row:** AN-25.4 detects generated flat forms' fields at or above the design's floor, which is the row's exit.

## AN-26 — Spell checking of fields and comments

**Spell checking of fields and comments** with host-supplied dictionaries

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'spellcheck|spell_check' crates tools` finds nothing

**Exit criterion.** a host dictionary flags exactly the misspellings planted in fixtures

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-26.1 | build | Spell checking of field values against a host-supplied dictionary: words split at the layout crate's word boundaries, misspellings reported with their field and position. | `crates/tinker-pdf/src/spelling.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/spelling.rs` (new), `docs/features/forms.md` | `spelling.rs` (new) `fields_flag_exactly_the_planted_misspellings`: a host dictionary flags exactly the misspellings planted in field fixtures. | — |
| AN-26.2 | build | Comments checked the same way: annotations' `/Contents` and rich text. | `crates/tinker-pdf/src/spelling.rs` (new), `crates/tinker-pdf/tests/spelling.rs` (new) | `comments_flag_exactly_the_planted_misspellings`: planted misspellings in comments are flagged exactly. | AN-26.1 |

**Closes the row:** AN-26.1 and AN-26.2 flag exactly the planted misspellings in fields and comments, which is the row's exit.

## AN-27 — XFA read

**XFA read**: the packets (template, datasets, config, localeSet, connectionSet, form, xfdf, sourceSet, stylesheet, signature) from an array or stream and stand-alone XDP; "pure XFA" detected; datasets exported and imported; XFA removed to fall back to AcroForm

**Today.** XFA was a named hard limit of the roadmap until 9 October 2026 — "removed in ISO 32000-2" (`features/forms.md:573`) — which is a reason about a standard and not about this repository's power: the specification is published free and deployed forms carry it; `sanitise` removes `/XFA` under `javascript`

**Exit criterion.** every packet of a generated XFA document read byte-equal; removal leaves a valid AcroForm document

**Doc:** `design/xfa.md` (owed) · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-27.1 | design | Create `docs/design/xfa.md` for AN-27 to AN-34: the packets and the XFA DOMs, data binding and SOM, the layout engine's model and its tie to the layout crate, FormCalc and the scripting object model over FJ's engine, picture clauses and locales, flattening and signatures, with each row's milestones. | `docs/design/xfa.md` (new) | The doc exists in the design-doc order, names this row's exit tests and the milestones AN-28 to AN-34 extend, and is reviewed. | — |
| AN-27.2 | build | The packets read from `/XFA` as an array of name and stream pairs or one stream, and from a stand-alone XDP, through the XML crate; pure XFA detected. | `crates/tinker-pdf/src/xfa/mod.rs` (new), `crates/tinker-pdf/src/xfa/packets.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/xfa_packets.rs` (new), `docs/features/forms.md` | `xfa_packets.rs` (new) `every_packet_reads_byte_equal`: every packet of generated XFA documents and XDP files reads byte-equal; pure XFA is detected. | AN-27.1 |
| AN-27.3 | build | Datasets exported and imported, and XFA removed to fall back to the AcroForm. | `crates/tinker-pdf/src/xfa/packets.rs` (new), `crates/tinker-pdf-cos/src/edit/sanitise.rs`, `crates/tinker-pdf/tests/xfa_packets.rs` (new) | `datasets_exchange_and_removal_leaves_an_acroform`: exported datasets import to the same packet; after removal the document validates strictly and its fields fill. | AN-27.2 |

**Closes the row:** AN-27.2 reads every packet byte-equal and AN-27.3 leaves a valid AcroForm document after removal, which is the row's exit.

## AN-28 — Static XFA

**Static XFA**: AcroForm widgets and XFA data kept in step when filling and saving

**Today.** absent — the owner's parity decision of 9 October 2026; XFA is only removed (`Sanitise`'s `XfaForm`, `crates/tinker-pdf-cos/src/edit/sanitise.rs:136-137`) and named by the archival and accessibility validators; no packet is read

**Exit criterion.** a fill through either model is visible in the other after save and reopen

**Doc:** `design/xfa.md` (owed) · **Depends on:** AN-27

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-28.1 | design | Add static XFA's milestones to `docs/design/xfa.md`: how a field's SOM name and bind map to its AcroForm name, and which side wins on save. | `docs/design/xfa.md` | The section names AN-28.2 and AN-28.3's exit tests and is reviewed. | AN-27.1 |
| AN-28.2 | build | Each static form's XFA fields mapped to their AcroForm fields. | `crates/tinker-pdf/src/xfa/static_form.rs` (new), `crates/tinker-pdf/tests/xfa_static.rs` (new) | `xfa_static.rs` (new) `every_field_maps`: generated static forms map every field both ways. | AN-28.1, AN-27 |
| AN-28.3 | build | Fills kept in step: an AcroForm fill written into the datasets on save, and a datasets fill applied to the AcroForm values and appearances. | `crates/tinker-pdf/src/xfa/static_form.rs` (new), `crates/tinker-pdf/tests/xfa_static.rs` (new), `docs/features/forms.md` | `a_fill_through_either_model_shows_in_the_other`: after save and reopen, a fill through either model is visible in the other. | AN-28.2 |

**Closes the row:** AN-28.3 makes a fill through either model visible in the other after save and reopen, which is the row's exit.

## AN-29 — The dynamic XFA layout engine

**The dynamic XFA layout engine**: template, data and form merge (data binding, SOM), flowed subforms (positioned, top-to-bottom, left-to-right, tables), pagination (page sets, page and content areas, occurrence, overflow leaders and trailers, keeps and breaks), draws, fields, exclusion groups, rich text, typeface mapping, barcode and image fields, instance managers

**Today.** absent — the owner's parity decision of 9 October 2026; no XFA packet is read (AN-28); `rg -i 'subform' crates` finds nothing

**Exit criterion.** layouts of the XFA specification's own examples reproduce its stated geometry, computed in the test; generated forms paginate deterministically on four targets; real government forms are a person's review (H-79)

**Doc:** `design/xfa.md` (owed) · **Depends on:** AN-27, CD-01

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-29.1 | design | Add the dynamic layout engine's milestones to `docs/design/xfa.md`: the template and form DOMs, merge and SOM, each layout strategy, pagination, rendering and instance managers, and how the specification's examples adjudicate geometry. | `docs/design/xfa.md` | The section names AN-29.2 to AN-29.15's exit tests and is reviewed. | AN-27.1 |
| AN-29.2 | build | The template DOM: subforms, fields, draws, areas and their properties parsed into a typed model. | `crates/tinker-pdf/src/xfa/template.rs` (new), `crates/tinker-pdf/tests/xfa_layout.rs` (new) | `xfa_layout.rs` (new) `templates_parse`: the specification's example templates parse to the stated structure. | AN-29.1, AN-27 |
| AN-29.3 | build | SOM expressions resolved over the DOMs: names, indices, wildcards and the shortcuts, in a module FormCalc and the scripting model share. | `crates/tinker-pdf/src/xfa/som.rs` (new), `crates/tinker-pdf/tests/xfa_layout.rs` (new) | `som_expressions_resolve`: the specification's SOM examples resolve to the stated nodes. | AN-29.2 |
| AN-29.4 | build | Data binding: template and data merged into the form DOM by normal, global and none binding and explicit binds. | `crates/tinker-pdf/src/xfa/merge.rs` (new), `crates/tinker-pdf/tests/xfa_layout.rs` (new) | `merges_give_the_stated_form`: the specification's merge examples give the stated form DOM. | AN-29.3 |
| AN-29.5 | build | Positioned layout: draws and fields placed by position and anchor, with margins, borders and captions. | `crates/tinker-pdf/src/xfa/layout.rs` (new), `crates/tinker-pdf/tests/xfa_layout.rs` (new) | `positioned_layout_reproduces_the_examples`: the examples' geometry is reproduced as computed in the test. | AN-29.4, CD-01 |
| AN-29.6 | build | Flowed subforms (top to bottom, left to right and right to left) and growable containers. | `crates/tinker-pdf/src/xfa/layout.rs` (new), `crates/tinker-pdf/tests/xfa_layout.rs` (new) | `flowed_layout_reproduces_the_examples`: the examples' geometry is reproduced. | AN-29.5 |
| AN-29.7 | build | Tables: table and row layouts with column widths and spans. | `crates/tinker-pdf/src/xfa/layout.rs` (new), `crates/tinker-pdf/tests/xfa_layout.rs` (new) | `tables_reproduce_the_examples`: the examples' geometry is reproduced. | AN-29.6 |
| AN-29.8 | build | Pagination: page sets, page and content areas, occurrence rules and overflow into the next content area. | `crates/tinker-pdf/src/xfa/paginate.rs` (new), `crates/tinker-pdf/tests/xfa_layout.rs` (new) | `pagination_reproduces_the_examples`: page counts and placements equal the examples'. | AN-29.6 |
| AN-29.9 | build | Overflow leaders and trailers, bookends, keeps and breaks. | `crates/tinker-pdf/src/xfa/paginate.rs` (new), `crates/tinker-pdf/tests/xfa_layout.rs` (new) | `leaders_keeps_and_breaks_reproduce_the_examples`: each example's pages equal the stated ones. | AN-29.8 |
| AN-29.10 | build | Rendering: draws (text, lines, rectangles, arcs, images) and fields written as PDF pages, fields as AcroForm widgets. | `crates/tinker-pdf/src/xfa/render.rs` (new), `crates/tinker-pdf/tests/xfa_layout.rs` (new) | `laid_out_forms_render`: each example's pages render with every object where the layout put it. | AN-29.7, AN-29.9 |
| AN-29.11 | build | Exclusion groups, and barcode (ED-49) and image fields. | `crates/tinker-pdf/src/xfa/render.rs` (new), `crates/tinker-pdf/tests/xfa_layout.rs` (new) | `exclusion_groups_barcodes_and_images`: groups behave as radios; barcodes decode through ED-50; images place as stated. | AN-29.10, ED-49, ED-50 |
| AN-29.12 | build | Rich text (`exData` XHTML) laid out through AN-16's path, and typeface mapping. | `crates/tinker-pdf/src/xfa/render.rs` (new), `crates/tinker-pdf/tests/xfa_layout.rs` (new) | `rich_text_and_typefaces`: rich text renders as the same XHTML laid out at its box; mapped typefaces are the ones the mapping names, in both builds. | AN-29.10, AN-16 |
| AN-29.13 | build | Instance managers: repeated subforms made from data records, with minimum, maximum and initial counts. | `crates/tinker-pdf/src/xfa/merge.rs` (new), `crates/tinker-pdf/tests/xfa_layout.rs` (new) | `instances_follow_the_data`: data with N records gives N instances within the limits, laid out as computed. | AN-29.10 |
| AN-29.14 | test | Determinism and robustness: generated forms fingerprinted on the four targets, and a fuzz target over templates and data. | `crates/tinker-pdf/tests/determinism.rs`, `fuzz/fuzz_targets/xfa_layout.rs` (new), `fuzz/Cargo.toml`, `docs/verification.md` | Generated forms paginate to the same fingerprint on every K-07 target; the fuzz target runs clean with the roster regenerated. | AN-29.11, AN-29.12, AN-29.13, K-07 |
| AN-29.15 | watch | Real government forms laid out and reviewed by a person. | `docs/features/forms.md` | A person's dated review under H-79 is recorded in the feature doc. | AN-29.14, H-79 |

**Closes the row:** AN-29.5 to AN-29.13 reproduce the specification's examples' geometry, AN-29.14 paginates generated forms deterministically on four targets, and AN-29.15 is the person's review, which is the row's exit.

## AN-30 — The XFA scripting object model

**The XFA scripting object model**: `xfa.host`, `form`, `layout`, `event`, `datasets`, `resolveNode(s)`, instance managers, `$record`, raw and formatted values, presence, mandatory and access

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'resolveNode|xfa\.host' crates tools` finds nothing

**Exit criterion.** expectations a person wrote (H-70) from the specification's script examples

**Doc:** `design/xfa.md` (owed) · **Depends on:** AN-29, FJ-03e, H-70

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-30.1 | design | Add the scripting object model's milestones to `docs/design/xfa.md`: its objects, their binding to FJ's engine, and the event processing order. | `docs/design/xfa.md` | The section names AN-30.2 to AN-30.7's exit tests and is reviewed. | AN-27.1 |
| AN-30.2 | build | `xfa.host`, `xfa.event` and `$record` bound to FJ's engine through its host objects. | `crates/tinker-pdf/src/xfa/script.rs` (new), `crates/tinker-pdf/tests/xfa_script.rs` (new) | `xfa_script.rs` (new) `host_event_and_record_behave_as_stated`: each member holds to the expectations a person wrote under H-70. | AN-30.1, AN-29, FJ-03e, H-70 |
| AN-30.3 | build | `xfa.form`, `resolveNode` and `resolveNodes` through AN-29.3's SOM, and `xfa.datasets`. | `crates/tinker-pdf/src/xfa/script.rs` (new), `crates/tinker-pdf/tests/xfa_script.rs` (new) | `form_and_resolution_behave_as_stated`: the specification's script examples hold. | AN-30.2 |
| AN-30.4 | build | `xfa.layout` from AN-29's results, and instance managers driven from script. | `crates/tinker-pdf/src/xfa/script.rs` (new), `crates/tinker-pdf/tests/xfa_script.rs` (new) | `layout_and_instances_behave_as_stated`: page counts, positions and instance changes hold to the expectations. | AN-30.3 |
| AN-30.5 | build | Raw and formatted values through AN-32's picture clauses, presence, mandatory and access. | `crates/tinker-pdf/src/xfa/script.rs` (new), `crates/tinker-pdf/tests/xfa_script.rs` (new) | `values_presence_and_access_behave_as_stated`: the expectations hold. | AN-30.3, AN-32 |
| AN-30.6 | build | Events dispatched in the processing order: initialize, calculate, validate, change, exit, click, pre-save and pre-print among them. | `crates/tinker-pdf/src/xfa/script.rs` (new), `crates/tinker-pdf/tests/xfa_script.rs` (new) | `events_fire_in_order`: event order and effects hold to the expectations. | AN-30.4, AN-30.5 |
| AN-30.7 | test | A coverage list of the model's members: each implemented and held to an expectation, or throwing a typed refusal naming itself. | `crates/tinker-pdf/tests/xfa_script.rs` (new) | The coverage test counts every listed member and none is `undefined`. | AN-30.6 |

**Closes the row:** AN-30.2 to AN-30.6 hold the model to the expectations a person wrote from the specification's script examples, which is the row's exit.

## AN-31 — FormCalc

**FormCalc**: grammar, SOM expressions and the built-in functions (arithmetic, date and time, financial, logical, string, URL calls as host requests)

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'formcalc' crates tools` finds nothing

**Exit criterion.** each built-in held to the XFA specification's stated examples; the grammar fuzzed (FJ-05)

**Doc:** `design/xfa.md` (owed) · **Depends on:** FJ-01

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-31.1 | design | Add FormCalc's milestones to `docs/design/xfa.md`: its grammar, evaluation over SOM through a host interface, the built-in families and URL calls as host requests. | `docs/design/xfa.md` | The section names AN-31.2 to AN-31.8's exit tests and is reviewed. | AN-27.1 |
| AN-31.2 | build | FormCalc's lexer and parser in FJ-01's script crate. | `crates/tinker-pdf-script/src/formcalc/parse.rs` (new), `crates/tinker-pdf-script/src/formcalc/mod.rs` (new) | Unit tests: the specification's grammar examples parse; malformed input is a typed error with its position. | AN-31.1, FJ-01 |
| AN-31.3 | build | The evaluator: expressions, control flow and function definitions, SOM references resolved through a host interface. | `crates/tinker-pdf-script/src/formcalc/eval.rs` (new) | Unit tests: the specification's evaluation examples give the stated values. | AN-31.2 |
| AN-31.4 | build | Arithmetic and logical built-ins. | `crates/tinker-pdf-script/src/formcalc/builtins.rs` (new) | Unit tests: each built-in holds to the specification's stated examples. | AN-31.3 |
| AN-31.5 | build | Date and time built-ins, with locales through AN-32. | `crates/tinker-pdf-script/src/formcalc/builtins.rs` (new) | Unit tests: each built-in holds to the stated examples. | AN-31.4, AN-32 |
| AN-31.6 | build | Financial built-ins. | `crates/tinker-pdf-script/src/formcalc/builtins.rs` (new) | Unit tests: each built-in holds to the stated examples. | AN-31.4 |
| AN-31.7 | build | String built-ins, and URL calls handed to the host as typed requests. | `crates/tinker-pdf-script/src/formcalc/builtins.rs` (new) | Unit tests: each built-in holds to the stated examples; URL calls yield their requests and perform no I/O. | AN-31.4 |
| AN-31.8 | test | The grammar fuzzed as FJ-05 fuzzes the language. | `fuzz/fuzz_targets/formcalc.rs` (new), `fuzz/Cargo.toml`, `docs/verification.md` | The target runs clean in the fuzz check and its seeds replay; the roster is regenerated. | AN-31.2, FJ-05.1 |

**Closes the row:** AN-31.4 to AN-31.7 hold each built-in to the specification's examples and AN-31.8 fuzzes the grammar, which is the row's exit.

## AN-32 — XFA picture clauses and locales

**XFA picture clauses and locales**: dates, times, numbers, text, zero and null patterns per locale set

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'picture_clause|picture clause' crates tools` finds nothing

**Exit criterion.** the specification's picture-clause examples formatted and parsed in the test

**Doc:** `design/xfa.md` (owed) · **Depends on:** AN-27

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-32.1 | design | Add picture clauses and locales to `docs/design/xfa.md`: the `localeSet` packet, default locales as the specification gives them, and each pattern family. | `docs/design/xfa.md` | The section names AN-32.2 to AN-32.4's exit tests and is reviewed. | AN-27.1 |
| AN-32.2 | build | Locale sets read: calendar and number symbols, currency and the date and time patterns. | `crates/tinker-pdf/src/xfa/locale.rs` (new), `crates/tinker-pdf/tests/xfa_pictures.rs` (new) | `xfa_pictures.rs` (new) `locale_sets_read`: generated locale sets read as written. | AN-32.1, AN-27 |
| AN-32.3 | build | Date and time picture clauses formatted and parsed. | `crates/tinker-pdf/src/xfa/picture.rs` (new), `crates/tinker-pdf/tests/xfa_pictures.rs` (new) | `dates_and_times_format_and_parse`: the specification's examples format and parse as stated. | AN-32.2 |
| AN-32.4 | build | Numeric, text, zero and null patterns formatted and parsed. | `crates/tinker-pdf/src/xfa/picture.rs` (new), `crates/tinker-pdf/tests/xfa_pictures.rs` (new) | `numbers_text_zero_and_null_format_and_parse`: the specification's examples format and parse as stated. | AN-32.3 |

**Closes the row:** AN-32.3 and AN-32.4 format and parse the specification's picture-clause examples, which is the row's exit.

## AN-33 — XFA flattened to static PDF

**XFA flattened to static PDF**, optionally with AcroForm fields, for archiving and printing

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'xfa_flatten|flatten_xfa' crates tools` finds nothing

**Exit criterion.** the flattened document renders as the dynamic layout drew it and passes PDF/A where asked

**Doc:** `design/xfa.md` (owed) · **Depends on:** AN-29

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-33.1 | design | Add flattening's milestones to `docs/design/xfa.md`: what static PDF the dynamic layout's pages become, with and without AcroForm fields, and the PDF/A option. | `docs/design/xfa.md` | The section names AN-33.2 to AN-33.4's exit tests and is reviewed. | AN-27.1 |
| AN-33.2 | build | XFA flattened: the dynamic layout's pages written as static content with the field values drawn, and the XFA removed. | `crates/tinker-pdf/src/xfa/flatten.rs` (new), `crates/tinker-pdf/tests/xfa_flatten.rs` (new), `docs/features/forms.md` | `xfa_flatten.rs` (new) `flattened_xfa_renders_as_the_layout_drew_it`: the flattened document renders as the dynamic layout drew it. | AN-33.1, AN-29 |
| AN-33.3 | build | Flattening with AcroForm fields kept at the laid-out positions. | `crates/tinker-pdf/src/xfa/flatten.rs` (new), `crates/tinker-pdf/tests/xfa_flatten.rs` (new) | `flattened_fields_fill`: the kept fields fill and render as the layout drew them. | AN-33.2 |
| AN-33.4 | build | PDF/A output where asked: an output intent, embedded fonts and metadata. | `crates/tinker-pdf/src/xfa/flatten.rs` (new), `crates/tinker-pdf/tests/xfa_flatten.rs` (new) | `flattened_xfa_passes_pdf_a`: the output passes the engine's PDF/A validation for the asked part. | AN-33.2 |

**Closes the row:** AN-33.2 renders the flattened document as the dynamic layout drew it and AN-33.4 passes PDF/A where asked, which is the row's exit.

## AN-34 — XFA signatures

**XFA signatures** over data and template, created with in-engine keys and verified

**Today.** absent — the owner's parity decision of 9 October 2026; XFA's `signature` packet is not read (AN-27)

**Exit criterion.** XML signatures the engine creates over generated packets with SG-01's and SG-02's keys verify through SG-13's verifier, and a packet changed after signing is reported

**Doc:** `design/xfa.md` (owed) · **Depends on:** SG-13, AN-27

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| AN-34.1 | design | Add XFA signatures to `docs/design/xfa.md`: XML signatures over data and template packets, canonicalisation, references and the `signature` packet. | `docs/design/xfa.md` | The section names AN-34.2 to AN-34.4's exit tests and is reviewed. | AN-27.1 |
| AN-34.2 | build | XML canonicalisation (Canonical XML 1.0 and 1.1, and exclusive) in the XML crate. | `crates/tinker-pdf-xml/src/c14n.rs` (new), `crates/tinker-pdf-xml/src/lib.rs` | Unit tests: the specifications' canonicalisation examples are reproduced byte for byte. | AN-34.1 |
| AN-34.3 | build | XML signatures created over generated data and template packets with SG-01's and SG-02's keys, written into the `signature` packet. | `crates/tinker-pdf/src/xfa/signature.rs` (new), `crates/tinker-pdf/tests/xfa_signatures.rs` (new) | `xfa_signatures.rs` (new) `created_signatures_verify`: each signature verifies through SG-13's verifier. | AN-34.2, AN-27, SG-01, SG-02, SG-13 |
| AN-34.4 | build | XFA signatures verified, a packet changed after signing reported. | `crates/tinker-pdf/src/xfa/signature.rs` (new), `crates/tinker-pdf/tests/xfa_signatures.rs` (new), `docs/features/signatures.md` | `a_changed_packet_is_reported`: changing a signed packet after signing yields the typed report; untouched packets verify. | AN-34.3 |

**Closes the row:** AN-34.3 creates signatures that verify through SG-13 and AN-34.4 reports a packet changed after signing, which is the row's exit.
