# Capability tier — standards

[The roadmap](../ROADMAP.md) says how a step is worked and what comes next.

Parity and more here means: validate **and convert** to every ISO profile,
check the object grammar against the published model, accessibility from
checking to remediation, PDF 2.0 complete with its extensions, and
e-invoices.

## SD-01 — PDF/A validation, complete

**PDF/A validation, complete**: the 39 staged rules and every machine-checkable clause of parts 1a/1b, 2a/2b/2u, 3a/3b/3u and 4/4e/4f, each finding with clause, test number and object path

**Exit criterion.** the staged count reaches zero one ledger class at a time; agreement ratcheted up and the false-positive count held at 830 of 831 or better by `pdfa_ledger.rs` nightly

**Doc:** [design/pdfa.md](../design/pdfa.md) · **Depends on:** MS-13

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-01.2 | test | A clause census: every clause of parts 1a/1b, 2a/2b/2u, 3a/3b/3u and 4/4e/4f listed from the archival validator's published profiles, commit cited, each with its running rule, its staged entry or its recorded reading. | `crates/tinker-pdf/tests/pdfa_clauses.rs` (new), `crates/tinker-pdf/tests/pdfa_clauses.tsv` (new), `crates/tinker-pdf/src/pdfa.rs` | `pdfa_clauses.rs` (new) fails on a clause with none of the three; each clause it finds unlisted is staged by name and added to the step of this row that owns its clause group. | — |
| SD-01.3 | build | Every finding carries its clause, the test number the archival validator's profile gives the rule, and an object path from the trailer, beside the `ObjRef` that `ConformanceFinding` holds today. | `crates/tinker-pdf/src/pdfa.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/pdfa_syntax.rs`, `docs/features/pdfa.md` | Every finding over the committed fixtures carries all three, asserted in each group's tests; `tpdf check --pdfa` prints them. | SD-01.2 |
| SD-01.4 | build | A raw-byte pass for what the lexer normalises away: a hexadecimal string with an odd digit count or a non-hexadecimal character (6.1.6), the EOL markers around `obj` and `endobj` (6.1.8), and the EOL after `stream` and before `endstream` (6.1.7). | `crates/tinker-pdf/src/pdfa/structure.rs`, `crates/tinker-pdf/src/pdfa/syntax.rs`, `crates/tinker-pdf/tests/pdfa_structure.rs`, `crates/tinker-pdf/tests/hostile_input.rs` | Each rule has a fixture and twin; the three staged entries are gone; `hostile_input.rs` runs the pass with zero panics. | — |
| SD-01.5 | build | What is left of the cross-reference syntax (6.1.4): subsection header spacing, a cross-reference stream in a part 1 file, and hybrid-reference files. | `crates/tinker-pdf/src/pdfa/structure.rs`, `crates/tinker-pdf/tests/pdfa_structure.rs` | Each has a fixture and twin; the staged entry is gone. | — |
| SD-01.6 | build | Each part's implementation limits (6.1.12): string and name length, integer and real magnitude, dictionary size and nesting depth, transcribed per part from the archival validator's published profiles, commit cited, with only the `shall` side run. | `crates/tinker-pdf/src/pdfa/syntax.rs`, `crates/tinker-pdf/tests/pdfa_syntax.rs` | Each limit has a fixture at its edge and one past it, per part; each `should` limit is a recorded reading; the staged entry is gone. | — |
| SD-01.7 | build | Two readings settled from the archival validator's published profiles and the corpus annotations, each written beside its rule: parts 2 and 3's information dictionary against the XMP packet (6.1.5), and what the DocMDP transform inside `/Perms` may say (6.1.12). | `crates/tinker-pdf/src/pdfa/xmp.rs`, `crates/tinker-pdf/src/pdfa/syntax.rs`, `crates/tinker-pdf/tests/pdfa_metadata.rs`, `crates/tinker-pdf/tests/pdfa_syntax.rs` | Each either runs with a fixture and twin or is a `reading` ledger row with a test that a conforming file is silent; both staged entries are gone. | — |
| SD-01.8 | build | The predefined schemas' remaining value types (6.6.2.3): Rational, URI, URL, GPSCoordinate, XPath, Locale, MIMEType, ProperName, AgentName, RenditionClass and a structure's own fields, each against the grammar the XMP specification prints, or against its shape where none is printed. | `crates/tinker-pdf/src/pdfa/xmp_schemas.rs`, `crates/tinker-pdf/tests/pdfa_metadata.rs` | Each type has a fixture and twin; a type read for shape only says so in its rule; the staged entry is gone. | — |
| SD-01.9 | build | An extension schema's second hop (6.7.8): a `pdfaProperty:valueType` naming a type that is described, and a type with fields not used where a simple value is declared; and the `pdfaid` schema described where the part requires it (6.7.11). | `crates/tinker-pdf/src/pdfa/xmp_extension.rs`, `crates/tinker-pdf/tests/pdfa_metadata.rs` | Each has a fixture and twin; both staged entries are gone. | — |
| SD-01.10 | build | Every metadata stream found, a font's, an image's, a form XObject's, and its packet header's `bytes` and `encoding` judged (6.6.2.1); part 1's filter prohibition (6.7.2) run under both readings over the corpus, the one that adds no false positive kept and the other recorded as a reading. | `crates/tinker-pdf/src/pdfa/xmp.rs`, `crates/tinker-pdf/tests/pdfa_metadata.rs`, `crates/tinker-pdf/tests/pdfa_ledger.tsv` | Each has a fixture and twin; on a dispatched run on `develop` the kept reading holds false positives at 830 of 831 or better; both staged entries are gone. | — |
| SD-01.11 | build | The content walk in `pdfa/content.rs` given what the interpreter knows, through the recording device: each inline image's dictionary, each drawn code with its font, glyph and rendering mode, and each marked-content property list. | `crates/tinker-pdf/src/pdfa/content.rs`, `crates/tinker-pdf-content/src/record.rs`, `crates/tinker-pdf/tests/pdfa_fonts.rs`, `crates/tinker-pdf/tests/hostile_input.rs` | The walk's records on committed fixtures equal counts made by hand in the tests; every running rule gives the findings it gave before; `hostile_input.rs` runs the walk with zero panics. | — |
| SD-01.12 | build | Inline image dictionaries (6.1.9) and the LZWDecode filter inside one (6.1.10), over the walk's records. | `crates/tinker-pdf/src/pdfa/syntax.rs`, `crates/tinker-pdf/tests/pdfa_syntax.rs` | Each has a fixture and twin in a page, a form and an appearance stream; both staged entries are gone. | SD-01.11 |
| SD-01.13 | build | The published Adobe Glyph List vendored as data under BSD-3-Clause, as the CMap resources are, with a name-to-Unicode lookup the PDF/A and PDF/UA font groups share. | `crates/tinker-pdf-font/data/agl` (new), `crates/tinker-pdf-font/src/agl.rs` (new), `crates/tinker-pdf-font/src/lib.rs`, `THIRDPARTY.md` | Every name on the list resolves to its code points in a test and a name off it does not; `THIRDPARTY.md` records the source and licence; `cargo deny` passes; both builds. | — |
| SD-01.14 | build | What a page draws, over the walk's records: a `.notdef` drawn, and a code with no Unicode mapping and no `/ActualText` (6.3.9); and a `/Differences` name off the glyph list drawn on the page (6.3.8). | `crates/tinker-pdf/src/pdfa/fonts.rs`, `crates/tinker-pdf/tests/pdfa_fonts.rs` | Each has a fixture and twin, the 6.3.8 pair built like the two corpus files its staged entry names, which carry the same fonts and differ in one name; both staged entries are gone. | SD-01.11, SD-01.13 |
| SD-01.15 | build | Level A's two content rules: a Private Use Area character with an `/ActualText` of its own (6.2.11.7.3), and a `/Lang` inside a marked-content property list (6.7.4). | `crates/tinker-pdf/src/pdfa/logical.rs`, `crates/tinker-pdf/tests/pdfa_logical.rs` | Each has a fixture and twin, the PUA rule's twins carrying an `/ActualText` for another character in the same run and one for a sequence; both staged entries are gone. | SD-01.11 |
| SD-01.16 | build | A form or appearance stream that names a resource has an explicitly associated `/Resources` dictionary (6.2.10, parts 2 to 4), over the walk's records. | `crates/tinker-pdf/src/pdfa/content.rs`, `crates/tinker-pdf/tests/pdfa_colour.rs` | A fixture and twin per stream kind; the staged entry is gone. | SD-01.11 |
| SD-01.18 | build | Dictionary prohibitions: `/Interpolate`, `/Alternates` and `/OPI` on an image (6.2.4); `/Ref`, PostScript XObjects, `/OPI` and `/Subtype2` on an XObject (6.2.5); `/TR`, `/TR2`, `/HTP` and the halftone constraints in a graphics state (6.2.8). | `crates/tinker-pdf/src/pdfa/colour.rs`, `crates/tinker-pdf/tests/pdfa_colour.rs` | Each prohibition has a fixture and twin under each part it applies to; the 6.2.5 and 6.2.8 entries are gone and 6.2.4's names only JPEG 2000. | — |
| SD-01.19 | build | The JPEG 2000 constraints parts 2 to 4 add (6.2.4), read from the codestream's own boxes and header through the decoder's readers. | `crates/tinker-pdf/src/pdfa/colour.rs`, `crates/tinker-pdf-filters/src/jpx/boxes.rs`, `crates/tinker-pdf-filters/src/jpx/codestream.rs`, `crates/tinker-pdf/tests/pdfa_colour.rs` | Each constraint has a fixture and twin built from committed codestreams; the 6.2.4 entry is gone. | SD-01.18 |
| SD-01.20 | build | Transparency under parts 2 to 4 (6.4): the blending colour space a group declares, isolation and knockout, and the soft masks those parts permit. | `crates/tinker-pdf/src/pdfa/colour.rs`, `crates/tinker-pdf/tests/pdfa_colour.rs` | Each has a fixture and twin per part; part 1's prohibition gives the findings it gave before; the staged entry is gone. | — |
| SD-01.21 | build | A destination profile judged past its header (6.2.2): a validating read, beside the transform builder, of the tag table, the tags each device class requires and their types, against the revision the header names (`docs/design/icc.md`). | `crates/tinker-pdf-color/src/icc.rs`, `crates/tinker-pdf/src/pdfa/colour.rs`, `crates/tinker-pdf/tests/pdfa_colour.rs` | Profiles built in the test and broken one tag at a time each give their finding and the intact one none; a well-formed profile the transform builder refuses gives none; the staged entry is gone. | — |
| SD-01.22 | build | Which font subtypes each part admits, part 4's prohibition on Type 3 among them (6.3.2), and a predefined CMap's character collection against the CIDFont's `/CIDSystemInfo` where a part lets it go unembedded (6.3.3), the collections carried as validation data. | `crates/tinker-pdf/src/pdfa/fonts.rs`, `crates/tinker-pdf-font/src/predefined.rs`, `crates/tinker-pdf/tests/pdfa_fonts.rs` | Each has a fixture and twin per part, with and without the `cmap-predefined` feature; both staged entries are gone. | — |
| SD-01.23 | build | Font metrics where the glyph is not reached by index (6.3.6): a Type 1 program's own advances, and a code a reader reaches only by 9.6.6.4's closing guess. | `crates/tinker-pdf/src/pdfa/fonts.rs`, `crates/tinker-pdf-font/src/type1.rs`, `crates/tinker-pdf/tests/pdfa_fonts.rs` | Each has a fixture and twin; the staged entry is gone. | SD-01.11 |
| SD-01.24 | build | Annotations: a `/Popup`'s `/Parent` and the `/AS` naming which state an appearance sub-dictionary shows (6.3.1), a widget's `/A` under parts 2 to 4 (6.9), and the annotation `/C` and `/IC` rule outside part 1 settled as a reading, since no later part states it. | `crates/tinker-pdf/src/pdfa/annotations.rs`, `crates/tinker-pdf/tests/pdfa_annotations.rs`, `crates/tinker-pdf/tests/pdfa_ledger.tsv` | Each running rule has a fixture and twin; a part 4 file with annotation colours is silent, with the reading beside its test; the three staged entries are gone. | — |
| SD-01.25 | build | Part 1's interactive form rules (6.9), over the appearance and font machinery their fixtures turn on. | `crates/tinker-pdf/src/pdfa/annotations.rs`, `crates/tinker-pdf/tests/pdfa_annotations.rs` | Each rule has a fixture and twin; part 1's half of the 6.9 entry is gone. | SD-01.24 |
| SD-01.26 | build | Part 4's additional actions, the entries permitted and those forbidden (6.5.2 and 6.6.3), and level E's action list (6.6.1), transcribed from the archival validator's published part 4 profile, commit cited. | `crates/tinker-pdf/src/pdfa/syntax.rs`, `crates/tinker-pdf/tests/pdfa_syntax.rs` | Each permitted and each forbidden entry has a fixture and twin under parts 4 and 4e; the three staged entries are gone. | — |
| SD-01.27 | build | Embedded files: the `/AF` back-reference from a document, page or annotation to its file specification, the MIME type an embedded file stream declares (6.8), and part 4's embedded-file rule under its 6.9. | `crates/tinker-pdf/src/pdfa/syntax.rs`, `crates/tinker-pdf/tests/pdfa_syntax.rs` | Each has a fixture and twin; that 6.8 entry and part 4's half of the 6.9 entry are gone. | — |
| SD-01.28 | build | Part 2's embedded files validated as PDF/A themselves (6.8): the validator called on each attachment's bytes, to a depth bound. | `crates/tinker-pdf/src/pdfa.rs`, `crates/tinker-pdf/tests/pdfa_syntax.rs`, `crates/tinker-pdf/tests/bounds_ledger.rs`, `crates/tinker-pdf/tests/hostile_input.rs` | A conforming and a non-conforming attachment each give the expected verdict; nesting past the bound gives a named abstention; the bound is a `bounds_ledger.rs` row; the staged entry is gone. | SD-01.27 |
| SD-01.29 | build | Optional-content configuration under parts 2 to 4 (6.10), alternate presentations and page transitions (6.11), and the document requirements dictionary (6.12). | `crates/tinker-pdf/src/pdfa/syntax.rs`, `crates/tinker-pdf/tests/pdfa_syntax.rs` | Each has a fixture and twin; the three staged entries are gone. | — |

## SD-02 — PDF/A conversion with fix-ups

**PDF/A conversion with fix-ups**: missing fonts embedded (FT-12), device colour converted or an output intent added (ED-06b, RC-19), transparency flattened for part 1 (FO-10), forbidden actions, annotations and flags removed or repaired, XMP identification and `/Info` synchronised (SD-23), LZW, encryption and external streams removed, non-conforming attachments converted or dropped, `/Lang` added, `/CIDSet` and `/CharSet` fixed, `.notdef` use and `/ToUnicode` repaired; a report of fixed and unfixable

**Exit criterion.** converted corpus files pass SD-01's validator at the requested level; each fix-up proven on a fixture built to need it; unfixable cases reported by name

**Doc:** `design/pdfa-conversion.md` (owed) · **Depends on:** SD-01, FT-12, ED-06b, FO-10, SD-23

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-02.1 | design | Create `docs/design/pdfa-conversion.md`: the fix-up for each finding class, their order, the cases that cannot be fixed and are named, and the check that converted files pass SD-01 at the requested level. | `docs/design/pdfa-conversion.md` (new) | The doc exists in the design-doc order, names SD-02.2 to SD-02.8's exit tests. | — |
| SD-02.2 | build | The conversion driver: validate, apply fix-ups, validate again, and name what cannot be fixed. | `crates/tinker-pdf/src/pdfa_convert.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/pdfa_convert.rs` (new) | `pdfa_convert.rs` (new): the driver stops when the validator is clean or only named unfixable findings remain. | SD-02.1 |
| SD-02.3 | build | Missing fonts embedded (FT-12), with `/CIDSet` and `/CharSet` made right. | `crates/tinker-pdf/src/pdfa_convert.rs` (new), `crates/tinker-pdf/tests/pdfa_convert.rs` (new) | A fixture built to need it converts and passes the validator at the requested level. | SD-02.2, FT-12.6 |
| SD-02.4 | build | Device colour converted (ED-06b) or an output intent added, RC-19's sRGB profile the default. | `crates/tinker-pdf/src/pdfa_convert.rs` (new), `crates/tinker-pdf/tests/pdfa_convert.rs` (new) | Fixtures built to need each route convert and pass the validator at the requested level. | SD-02.2, ED-06b.4, ED-06b.5, ED-06b.8, RC-19.3 |
| SD-02.5 | build | Transparency flattened for part 1 (FO-10). | `crates/tinker-pdf/src/pdfa_convert.rs` (new), `crates/tinker-pdf/tests/pdfa_convert.rs` (new) | A transparent fixture converts to part 1 and passes the validator. | SD-02.2, FO-10 |
| SD-02.6 | build | Forbidden actions, annotations and flags removed or repaired; LZW recompressed; encryption removed; external streams internalised (ED-33). | `crates/tinker-pdf/src/pdfa_convert.rs` (new), `crates/tinker-pdf/tests/pdfa_convert.rs` (new) | Fixtures built to need each fix-up convert and pass the validator at the requested level. | SD-02.2, ED-33 |
| SD-02.7 | build | XMP identification and `/Info` synchronised (SD-23), `/Lang` added, and non-conforming attachments converted or dropped. | `crates/tinker-pdf/src/pdfa_convert.rs` (new), `crates/tinker-pdf/tests/pdfa_convert.rs` (new) | Fixtures built to need each fix-up convert and pass the validator at the requested level. | SD-02.2, SD-23 |

## SD-03 — An archival profile on rewrite

**An archival profile on rewrite** — the rewrite path can refuse

**Exit criterion.** a rewrite under a profile refuses by clause what the builder refuses; outputs pass SD-01

**Doc:** — · **Depends on:** H-19 (the API change)

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-03.1 | build | `rewrite` under an archival profile returns a result carrying refusals by clause, reusing the builder's refusals, through H-19's API change before the freeze. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf/tests/pdfa_writer.rs`, `docs/features/pdfa.md` | In `pdfa_writer.rs`, each refusal the builder makes has a rewrite twin refused under the same clause, and each rewrite output passes the validator as it stands. | H-19 |
| SD-03.2 | test | Rewrite outputs judged again once the validator is complete. | `crates/tinker-pdf/tests/pdfa_writer.rs` | Every rewrite fixture's output passes SD-01's validator with `PDFA_STAGED` empty. | SD-03.1, SD-01 |

## SD-04 — PDF/UA validation, complete

**PDF/UA validation, complete**: the table grid (milestone 3's other half), part 2's annotations and 7.18's residue (milestone 4), real content (5), UA-2 specifics (6), the ledger and the writer's claim (7), the font clauses that need the glyph list as data; WTPDF 1.0's accessibility and reuse conformance

**Exit criterion.** a rule group that decides the decidable clauses and abstains by name on the rest: the next nightly `pdfua.rs` records caught, abstained and false-alarm counts — the false alarms first, held at zero over the 195 conforming files; each new rule a fixture and twin — schedulable as M halves, one per milestone left

**Doc:** [design/pdfua.md](../design/pdfua.md) · **Depends on:** MS-13, H-41 (the writer's claim)

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-04.2 | build | The attribute reader in `structure.rs`: `/A`, `/ID`, `/Headers`, `/Scope`, `/RowSpan`, `/ColSpan` and `/ListNumbering`, shared with FT-31's table reconstruction. | `crates/tinker-pdf/src/structure.rs`, `crates/tinker-pdf/tests/tagged_pdf.rs` | Each attribute reads as written on the committed tagged fixtures; `tagged.elements` in `corpus/ratchet.json` is unchanged. | — |
| SD-04.3 | build | Milestone 3's other half, the table grid: intersecting cells, rows and columns across spans, and header association. | `crates/tinker-pdf/src/pdfua/structure.rs`, `crates/tinker-pdf/tests/pdfua_rules.rs` | Each rule has a fixture and twin; no false alarm on the conforming fixtures. | SD-04.2 |
| SD-04.4 | build | Milestone 4's rest: part 2's annotations (8.9, 8.10, 8.14) and 7.18's residue, `Form` roles, media clips and `Popup`. | `crates/tinker-pdf/src/pdfua/annotations.rs`, `crates/tinker-pdf/tests/pdfua_rules.rs` | Each rule has a fixture and twin, every pass-twin exemption asserted by name; the `FREETEXT` fixtures stay silent. | — |
| SD-04.5 | build | Milestone 5: the real-content group over the recording device, for 7.1-t01 to t03, 8.2.2 and 7.20-t02. | `crates/tinker-pdf/src/pdfua.rs`, `crates/tinker-pdf-content/src/record.rs`, `crates/tinker-pdf/tests/pdfua_rules.rs` | Each has a fixture and twin with the two `-pass-` exemptions asserted; a counted injection recording under `Capture::GLYPHS` instead fails the group's tests. | — |
| SD-04.6 | build | Milestone 6's first half: `pdfuaid:rev`, Private Use Area code points, structure destinations, `Link` targets and `AFRelationship`. | `crates/tinker-pdf/src/pdfua/syntax.rs`, `crates/tinker-pdf/src/pdfua/structure.rs`, `crates/tinker-pdf/tests/pdfua_rules.rs` | Each has a fixture and twin. | SD-04.5 |
| SD-04.7 | build | Milestone 6's second half: the namespaced role-map rules and the 2.0 namespace on `Document` (8.2.4-t02-c, -t03, 8.2.5.2-t02), over the namespaces the structure reader now holds. | `crates/tinker-pdf/src/pdfua/structure.rs`, `crates/tinker-pdf/tests/pdfua_rules.rs` | Each has a fixture and twin; 8.2's and 8.4's abstentions shrink to the undecidable residue. | — |
| SD-04.8 | build | 7.21's and 8.4's font residue over the PDF/A font group's new rules: every drawn glyph present in the program, CharSet and CIDSet complete, Type 1 widths, a TrueType program's cmap subtables, glyph-list names, `/ToUnicode` at rendering mode 3, no `.notdef` drawn, and a predefined CMap's collection. | `crates/tinker-pdf/src/pdfua/fonts.rs`, `crates/tinker-pdf/tests/pdfua_rules.rs` | Each has a fixture and twin under both parts; the 7.21 and 8.4 staged entries name nothing the PDF/A group decides. | SD-01.13, SD-01.14, SD-01.22, SD-01.23 |
| SD-04.9 | build | WTPDF 1.0's accessibility and reuse conformance: each requirement mapped onto a PDF/UA-2 rule, a rule of its own or a named abstention, and both levels reported in the verdict. | `crates/tinker-pdf/src/pdfua.rs`, `crates/tinker-pdf/tests/pdfua_rules.rs`, `docs/features/pdfua.md` | Every requirement is mapped; each rule of its own has a fixture and twin; a fixture claiming each level gets the expected verdict. | SD-04.6 |
| SD-04.10 | build | Milestone 7's ledger: `pdfua_ledger.tsv` with uncovered and stale assertions, every residue file of milestones 2 to 6 a row with its reason. | `crates/tinker-pdf/tests/pdfua_ledger.tsv` (new), `crates/tinker-pdf/tests/pdfua.rs` | A row without a reason is refused by a test; on a dispatched run on `develop`, zero uncovered and zero stale. | SD-04.3, SD-04.4, SD-04.5, SD-04.7, SD-04.8, SD-04.9 |
| SD-04.11 | build | `pdfuaid` on the archival profile, stamped only as H-41 decides, and the tagged writer's own findings closed: the link with no `/Contents` and the page with no `/Tabs` that `pdfua.rs` names. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/pdfa_writer.rs`, `crates/tinker-pdf/tests/tagged_writer.rs` | Every `PageBuilder::tagged` fixture in `pdfa_writer.rs` validates under PDF/UA-1 with zero findings and its untagged twin does not. | H-41 |
| SD-04.13 | test | A second reading of every rule against ISO 14289-1 and -2 once bought. | `crates/tinker-pdf/src/pdfua.rs`, `crates/tinker-pdf/tests/pdfua_rules.rs`, `crates/tinker-pdf/tests/pdfua_ledger.tsv` (new) | Each rule's clause is re-read against the text; each disagreement is a fix with its fixture and twin or a `reading` ledger row. | H-96, SD-04.10 |

## SD-05 — An accessibility checker that lists what a person must check

**An accessibility checker that lists what a person must check**: every human-only condition named with page and element pointers instead of passing it; the failure-condition catalogue and WCAG 2.x success criteria mapped

**Exit criterion.** every catalogue condition mapped to a rule, an abstention or a named person's check; the report lists each instance in fixtures built to raise them

**Doc:** — · **Depends on:** SD-04

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-05.1 | build | Human-only conditions named with page and element pointers, every condition of the failure-condition catalogue mapped to a rule, an abstention or a named person's check. | `crates/tinker-pdf/src/accessibility.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/accessibility_checker.rs` (new) | `accessibility_checker.rs` (new): every catalogue condition is mapped; fixtures built to raise each list every instance with its pointers. | SD-04 |
| SD-05.2 | build | WCAG 2.x success criteria mapped onto the same report. | `crates/tinker-pdf/src/accessibility.rs` (new), `crates/tinker-pdf/tests/accessibility_checker.rs` (new) | Each criterion maps to rules, abstentions or person's checks; fixtures list each instance. | SD-05.1 |
| SD-05.3 | build | `tpdf check --accessibility`. | `tools/tpdf/src/main.rs`, `docs/features/pdfua.md` | A CLI test prints the report the facade gives. | SD-05.2 |

## SD-06 — Colour contrast measured by the renderer

**Colour contrast measured by the renderer**: text against its rendered background, per run, against WCAG's 4.5:1 and 3:1 thresholds

**Exit criterion.** contrast ratios computed in the test from the fixtures' colours equal the reported ones

**Doc:** — · **Depends on:** SD-05

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-06.1 | build | Contrast per text run: the run's colour against the background the renderer draws under its glyphs, by WCAG's relative luminance. | `crates/tinker-pdf/src/accessibility.rs` (new), `crates/tinker-pdf/tests/contrast.rs` (new) | `contrast.rs` (new): ratios computed in the test from the fixtures' colours equal the reported ones. | SD-05 |
| SD-06.2 | build | WCAG's 4.5:1 and 3:1 thresholds, large text by size and weight, reported through SD-05's checker. | `crates/tinker-pdf/src/accessibility.rs` (new), `crates/tinker-pdf/tests/contrast.rs` (new) | Runs below their threshold are reported and runs above are not. | SD-06.1 |

## SD-07 — Auto-tagging untagged documents

**Auto-tagging untagged documents**: a structure tree built from the inferred reading order, tables, headings, lists, figures and notes; running heads and feet as artifacts; parent tree, role map, `/Lang` per span; alternate-text placeholders flagged for a person; `/Tabs /S`; annotation references; an outline from headings

**Exit criterion.** committed tagged documents with their trees removed and auto-tagged score against their own trees above the floors the design states; outputs pass SD-04's machine-checkable rules

**Doc:** `design/auto-tagging.md` (owed) · **Depends on:** FT-30, FT-31

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-07.1 | design | Create `docs/design/auto-tagging.md`: a structure tree from FT-30's order and regions and FT-31's tables, headings, lists, figures and notes, artifacts, the parent tree and role map, `/Lang` per span, alternate-text placeholders flagged for a person, and scoring against committed tagged documents' own trees with stated floors. | `docs/design/auto-tagging.md` (new) | The doc exists in the design-doc order, states the floors, names SD-07.2 to SD-07.8's exit tests. | FT-30, FT-31 |
| SD-07.2 | build | The skeleton: marked content inserted through ED-01's page objects, paragraphs in the inferred order, and the parent tree. | `crates/tinker-pdf/src/autotag/mod.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/autotag.rs` (new) | `autotag.rs` (new): stripped committed documents auto-tag with paragraph order scoring above the stated floor. | SD-07.1, ED-01.7, ED-01.17, ED-01.18 |
| SD-07.3 | build | Headings, lists and notes. | `crates/tinker-pdf/src/autotag/blocks.rs` (new), `crates/tinker-pdf/tests/autotag.rs` (new) | Heading, list and note scores clear their floors. | SD-07.2 |
| SD-07.4 | build | Tables from FT-31 with spans and headers. | `crates/tinker-pdf/src/autotag/tables.rs` (new), `crates/tinker-pdf/tests/autotag.rs` (new) | Table scores clear their floor. | SD-07.2 |
| SD-07.5 | build | Figures with alternate-text placeholders flagged for a person, and running heads and feet as artifacts. | `crates/tinker-pdf/src/autotag/blocks.rs` (new), `crates/tinker-pdf/tests/autotag.rs` (new) | Figure and artifact scores clear their floors; every placeholder is flagged. | SD-07.2 |
| SD-07.6 | build | `/Lang` per span, the role map, `/Tabs /S`, annotation references, and an outline from the headings. | `crates/tinker-pdf/src/autotag/mod.rs` (new), `crates/tinker-pdf/tests/autotag.rs` (new) | Each is present and correct on the scored documents; the outline equals the headings. | SD-07.3 |
| SD-07.7 | test | The scoring harness over every committed tagged document, and SD-04's rules on the output. | `crates/tinker-pdf/tests/autotag.rs` (new) | Every score clears the design's floor; outputs pass SD-04's machine-checkable rules. | SD-07.4, SD-07.5, SD-07.6, SD-04 |
| SD-07.8 | build | `tpdf tag`. | `tools/tpdf/src/main.rs`, `docs/features/pdfua.md` | The command tags `testdata/` fixtures to the facade's output. | SD-07.7 |

## SD-08 — Structure-tree editing

**Structure-tree editing**: elements added, removed, moved and re-roled; `/Alt`, `/ActualText`, `/E`, `/Lang` and `/T` set; content made artifact and back; marked-content identifiers bound; table headers, scopes and IDs; list attributes; namespaces; the parent tree kept consistent

**Exit criterion.** each edit re-read through the structure view; SD-04's rules pass after every edit on the committed tagged fixtures

**Doc:** — · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-08.1 | build | Elements added, removed, moved and re-roled, with the parent tree kept consistent. | `crates/tinker-pdf/src/structure_edit.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/structure_editing.rs` (new) | `structure_editing.rs` (new): each edit re-reads through the structure view, and SD-04's rules pass after every edit on the committed tagged fixtures. | — |
| SD-08.2 | build | `/Alt`, `/ActualText`, `/E`, `/Lang` and `/T` set; content made artifact and back through ED-01; marked-content identifiers bound. | `crates/tinker-pdf/src/structure_edit.rs` (new), `crates/tinker-pdf/tests/structure_editing.rs` (new) | Each edit re-reads and SD-04's rules pass after it. | SD-08.1, ED-01.7, ED-01.17, ED-01.18 |
| SD-08.3 | build | Table headers, scopes and IDs, list attributes, and namespaces. | `crates/tinker-pdf/src/structure_edit.rs` (new), `crates/tinker-pdf/tests/structure_editing.rs` (new), `docs/features/pdfua.md` | Each edit re-reads and SD-04's rules pass after it. | SD-08.1, SD-04.2 |

## SD-09 — An accessibility tree and a screen-reader preview

**An accessibility tree and a screen-reader preview**: roles, names, states, order and boxes from tags or inference, platform-neutral for hosts, and a textual preview

**Exit criterion.** the tree of each committed tagged document lists its elements in order with the expected roles

**Doc:** — · **Depends on:** SD-07

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-09.1 | build | An accessibility tree from the tags, or from SD-07's inference when untagged: roles, names, states, order and boxes, in a platform-neutral form. | `crates/tinker-pdf/src/accessibility_tree.rs` (new), `crates/tinker-pdf/tests/accessibility_tree.rs` (new) | `accessibility_tree.rs` (new): each committed tagged document's tree lists its elements in order with the expected roles. | SD-07 |
| SD-09.2 | build | A textual screen-reader preview and `tpdf a11y-tree`. | `crates/tinker-pdf/src/accessibility_tree.rs` (new), `tools/tpdf/src/main.rs`, `docs/features/pdfua.md` | The preview reads each fixture's elements in order with their names; the command prints it. | SD-09.1 |

## SD-10 — Accessible output by construction

**Accessible output by construction**: the builder, HTML and Markdown paths emit tagged PDF/UA-1 and UA-2 by default, and a conformance-enforcing writer refuses what would break the claim

**Exit criterion.** every builder, HTML and Markdown fixture's output passes SD-04's machine-checkable rules; the claim is stamped only as H-41 decides

**Doc:** — · **Depends on:** SD-04, H-41

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-10.1 | build | The builder emits tagged PDF/UA-1 and UA-2 by default. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/tagged_writer.rs` | In `tagged_writer.rs`, every builder fixture's output passes the PDF/UA validator as it stands. | — |
| SD-10.2 | build | The HTML and Markdown paths emit tagged PDF/UA by default. | `crates/tinker-pdf/src/html.rs`, `crates/tinker-pdf/src/markdown.rs`, `crates/tinker-pdf/tests/html_creation.rs`, `crates/tinker-pdf/tests/markdown.rs` | Every HTML and Markdown fixture's output passes the PDF/UA validator as it stands. | SD-10.1 |
| SD-10.3 | build | A conformance-enforcing writer that refuses what would break the claim, the claim stamped through SD-04.11's profile. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/tagged_writer.rs`, `docs/features/pdfua.md` | Each breaking operation is refused by a typed error; the claim appears only where H-41's decision allows. | SD-10.2, SD-04.11 |
| SD-10.4 | test | Every builder, HTML and Markdown fixture judged by the complete PDF/UA rules. | `crates/tinker-pdf/tests/tagged_writer.rs`, `crates/tinker-pdf/tests/html_creation.rs`, `crates/tinker-pdf/tests/markdown.rs` | Every such fixture's output passes SD-04's machine-checkable rules. | SD-10.3, SD-04 |

## SD-11 — PDF/X validation and writing, complete

**PDF/X validation and writing, complete**: the 2003 levels measured; the writer's PDF/X flavour (trim and bleed boxes on the builder, output intents); X-4, X-4p, X-5g/n/pg and X-6/6n/6p; X-1a:2001 and X-3:2002; 19005-2's multiple-intent question; the colour registry looked up from bytes the host supplies

**Exit criterion.** `pdfx_census.rs` silent on every non-claiming corpus file; an ISO 15930 rule group per level held to fixtures and twins and to a third party's file per level where one exists; the archival profile grows a PDF/X flavour and the writer's output validates at its claimed level

**Doc:** [design/pdfx.md](../design/pdfx.md) · **Depends on:** MS-13, H-93, H-97, H-101

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-11.2 | ci | Milestone 2: the two published output suites pinned in `corpora.lock` with their licence recorded and `redistribute = no`. | `corpus/corpora.lock`, `crates/tinker-pdf/tests/pdfx_census.rs` | In CI, `cargo xtask corpus-fetch` verifies both by sha256 and the census asserts each file claims the flavour its suite says. | H-93 |
| SD-11.4 | build | Milestone 4's open question settled: whether ISO 19005-2's rule on more than one output intent covers every entry that carries a profile or only PDF/A's, read from the clause text the archival validator's published profile quotes, and the PDF/A colour group's skip of non-`GTS_PDFA1` intents kept or changed to match. | `crates/tinker-pdf/src/pdfa/colour.rs`, `crates/tinker-pdf/src/pdfx/print.rs`, `crates/tinker-pdf/tests/pdfx_rules.rs`, `docs/design/pdfx.md` | A fixture claiming PDF/A-2b and PDF/X-4 with one shared profile is silent under both validators, its twin with two profiles gets the finding the reading gives, and the reading is written beside the rule. | — |
| SD-11.5 | build | Milestone 6's writer flavour: `Standard` on the archival profile, `PageBuilder::set_trim_box` and the bleed box, `/Trapped`, the identification keys, the `GTS_PDFX` intent, and a typed refusal per forbidden feature. *Note:* The output intent embeds the profile the caller supplies; none is bundled (H-116 rules on output-intent profiles). | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/pdfx_writer.rs` (new), `docs/features/pdfx.md` | `pdfx_writer.rs` (new): each flavour's fixture is silent under this validator and the strict validator, each twin is not, and each forbidden feature has a refusal test. | — |
| SD-11.6 | test | The writer's third leg: each flavour's output compared for shape against a pinned suite file, and a document claiming PDF/A-2b and PDF/X-4 together. | `crates/tinker-pdf/tests/pdfx_writer.rs` (new) | Each shape comparison holds where the corpus is present; the double claim gets zero findings from the PDF/A and PDF/X validators. | SD-11.2, SD-11.4, SD-11.5 |
| SD-11.7 | build | Rule groups for PDF/X-1a:2001 and PDF/X-3:2002 from the CGATS notes Version 3, each rule citing its section. | `crates/tinker-pdf/src/pdfx/syntax.rs`, `crates/tinker-pdf/src/pdfx/print.rs`, `crates/tinker-pdf/tests/pdfx_rules.rs` | Each rule has a fixture and twin under both levels; each clause the notes do not restate is an `Unread` gap by name. | H-101 |
| SD-11.8 | build | The clauses of ISO 15930-4 and -6 the notes do not restate, each an `Unread` gap today: 6.1, 6.4, 6.7, 6.9, 6.12, 6.14, 6.15, 6.17, TrapNet, Table 2's shapes, JPEG 2000 and X-1a's device-independent colour, the text read twice against the rules first. | `crates/tinker-pdf/src/pdfx/syntax.rs`, `crates/tinker-pdf/src/pdfx/print.rs`, `crates/tinker-pdf/tests/pdfx_rules.rs` | Each clause runs with a fixture and twin under both 2003 levels or is a reading written beside its title; no 2003 clause is `Unread`. | H-97 |
| SD-11.9 | build | Milestone 7's X-4 and X-4p groups from ISO 15930-7, the text read twice against the rules first, X-4p's external profile reference included. | `crates/tinker-pdf/src/pdfx.rs`, `crates/tinker-pdf/src/pdfx/print.rs`, `crates/tinker-pdf/tests/pdfx_rules.rs` | Each clause runs with a fixture and twin or is a reading by title; X-4p's version string is identified from the text; no 15930-7 clause is `Unread`. | H-97 |
| SD-11.10 | build | The X-6, X-6n and X-6p groups from ISO 15930-9, the text read twice against the rules first. | `crates/tinker-pdf/src/pdfx.rs`, `crates/tinker-pdf/src/pdfx/print.rs`, `crates/tinker-pdf/tests/pdfx_rules.rs` | Each clause runs with a fixture and twin or is a reading by title; the three version strings are identified from the text. | H-97, SD-11.9 |
| SD-11.11 | build | The X-5g, X-5n and X-5pg groups from ISO 15930-8, the text read twice against the rules first. | `crates/tinker-pdf/src/pdfx.rs`, `crates/tinker-pdf/src/pdfx/print.rs`, `crates/tinker-pdf/tests/pdfx_rules.rs` | Each clause runs with a fixture and twin or is a reading by title; `PDF/X-5g` and its siblings are identified rather than carried as unknown claims. | H-160, SD-11.9 |
| SD-11.12 | build | An output intent naming a registered characterization rather than embedding a profile, looked up from registry bytes the host supplies and never fetched. | `crates/tinker-pdf/src/pdfx/print.rs`, `crates/tinker-pdf/tests/pdfx_rules.rs`, `docs/features/pdfx.md` | An intent naming an entry the host supplies is judged against that characterization; one naming an absent entry is a named abstention; with no registry supplied the verdict says so. | — |

## SD-12 — PDF/X conversion with fix-ups

**PDF/X conversion with fix-ups**: colour to the output intent, fonts embedded, boxes set, transparency flattened for X-1a and X-3, forbidden content removed

**Exit criterion.** converted fixtures pass SD-11 at the requested level

**Doc:** [design/pdfx.md](../design/pdfx.md) · **Depends on:** SD-11, ED-06b, FO-10, FT-12

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-12.1 | design | Amend `docs/design/pdfx.md` with conversion: the fix-ups per level and their order. | `docs/design/pdfx.md` | The amended doc names SD-12.2 to SD-12.5's exit tests. | SD-11.6, SD-11.7, SD-11.8, SD-11.10, SD-11.11, SD-11.12 |
| SD-12.2 | build | The conversion driver for PDF/X levels: validate, fix, validate again. | `crates/tinker-pdf/src/pdfx_convert.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/pdfx_convert.rs` (new) | `pdfx_convert.rs` (new): the driver stops clean or with named unfixable findings. | SD-12.1 |
| SD-12.3 | build | Colour to the output intent (ED-06b), fonts embedded (FT-12) and boxes set. | `crates/tinker-pdf/src/pdfx_convert.rs` (new), `crates/tinker-pdf/tests/pdfx_convert.rs` (new) | Fixtures needing each convert and pass SD-11 at the requested level. | SD-12.2, ED-06b.4, ED-06b.5, ED-06b.8, FT-12.6 |
| SD-12.4 | build | Transparency flattened for X-1a and X-3 (FO-10), and forbidden content removed. | `crates/tinker-pdf/src/pdfx_convert.rs` (new), `crates/tinker-pdf/tests/pdfx_convert.rs` (new) | Fixtures needing each convert and pass SD-11 at the requested level. | SD-12.2, FO-10 |
| SD-12.5 | build | `tpdf convert --pdfx`. | `tools/tpdf/src/main.rs`, `docs/features/pdfx.md` | The command converts `testdata/` fixtures that pass SD-11 at the requested level. | SD-12.3, SD-12.4 |

## SD-13 — PDF/E-1

**PDF/E-1** validation

**Exit criterion.** each rule held to a fixture and twin transcribed from ISO 24517-1

**Doc:** — · **Depends on:** H-102

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-13.1 | build | The PDF/E-1 validator's frame: the claim read as ISO 24517-1 states it, verdict types, `Document::validate_pdfe` and `tpdf check --pdfe`, the PDF/A kernel shared as the PDF/X and PDF/UA validators share it, and every clause listed as `Unread` until a step below runs it. | `crates/tinker-pdf/src/pdfe.rs` (new), `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/pdfe_rules.rs` (new), `crates/tinker-pdf/tests/hostile_input.rs` | `pdfe_rules.rs` (new): a unit fixture per identification form; every clause is in the verdict by name; `hostile_input.rs` calls the validator with zero panics. | H-102 |
| SD-13.2 | build | File structure, metadata and security clauses, each transcribed from ISO 24517-1 with its clause cited. | `crates/tinker-pdf/src/pdfe.rs` (new), `crates/tinker-pdf/tests/pdfe_rules.rs` (new) | Each rule has a fixture and twin; those clauses leave `Unread`. | SD-13.1 |
| SD-13.3 | build | Graphics, colour, transparency and font clauses, each transcribed with its clause cited. | `crates/tinker-pdf/src/pdfe.rs` (new), `crates/tinker-pdf/tests/pdfe_rules.rs` (new) | Each rule has a fixture and twin; those clauses leave `Unread`. | SD-13.1 |
| SD-13.4 | build | The engineering clauses: 3D artwork, multimedia, optional content and views, judged by their dictionaries, each transcribed with its clause cited. | `crates/tinker-pdf/src/pdfe.rs` (new), `crates/tinker-pdf/tests/pdfe_rules.rs` (new) | Each rule has a fixture and twin; those clauses leave `Unread`. | SD-13.1 |
| SD-13.5 | build | Annotation, action, form and JavaScript clauses, each transcribed with its clause cited. | `crates/tinker-pdf/src/pdfe.rs` (new), `crates/tinker-pdf/tests/pdfe_rules.rs` (new) | Each rule has a fixture and twin; those clauses leave `Unread`. | SD-13.1 |
| SD-13.6 | doc | `docs/features/pdfe.md` (new): every clause with its rule or the reading beside it. | `docs/features/pdfe.md` (new), `docs/README.md` | No clause is `Unread`, asserted in `pdfe_rules.rs`; the doc lists every clause. | SD-13.2, SD-13.3, SD-13.4, SD-13.5 |

## SD-14 — PDF/VT-1, 2 and 3

**PDF/VT-1, 2 and 3**: the document-part hierarchy (`/DPartRoot`, `/DParts`, record metadata) validated and written

**Exit criterion.** as SD-13, from ISO 16612-2/3

**Doc:** — · **Depends on:** H-102

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-14.1 | build | The document-part hierarchy read: `/DPartRoot`, `/DParts`, each part's `/DPM` record metadata and its page range, as ISO 16612-2 and the object-grammar model state them, cited. | `crates/tinker-pdf/src/document_parts.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/document_parts.rs` (new) | `document_parts.rs` (new): a hand-built hierarchy reads back node by node with its metadata and pages; a cycle or a page range out of order is a typed error. | H-102 |
| SD-14.2 | build | The hierarchy written: `DocumentBuilder` grows document parts with record metadata. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/document_parts.rs` (new) | Each written hierarchy reads back through SD-14.1's reader and passes the strict validator. | SD-14.1 |
| SD-14.3 | build | PDF/VT-1 validation from ISO 16612-2: the claim, the document-part rules and the record metadata's shape, over SD-11.9's PDF/X-4 base. | `crates/tinker-pdf/src/pdfvt.rs` (new), `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/pdfvt_rules.rs` (new) | `pdfvt_rules.rs` (new): each rule has a fixture and twin; a VT-1 claim on a file failing X-4 reports the X-4 findings. | SD-14.1, SD-11.9 |
| SD-14.4 | build | PDF/VT-2 and VT-2s from ISO 16612-2: external content over SD-11.11's PDF/X-5 base, and the streamed form. | `crates/tinker-pdf/src/pdfvt.rs` (new), `crates/tinker-pdf/tests/pdfvt_rules.rs` (new) | Each rule has a fixture and twin. | SD-14.3, SD-11.11 |
| SD-14.5 | build | PDF/VT-3 from ISO 16612-3 over SD-11.10's PDF/X-6 base. | `crates/tinker-pdf/src/pdfvt.rs` (new), `crates/tinker-pdf/tests/pdfvt_rules.rs` (new) | Each rule has a fixture and twin. | SD-14.3, SD-11.10 |
| SD-14.6 | build | The writer's PDF/VT flavour: a VT-1 file with its parts and record metadata written through SD-14.2 on SD-11.5's PDF/X flavour. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/pdfvt_rules.rs` (new) | Written VT-1 files validate under SD-14.3 with zero findings and each twin missing one requirement does not. | SD-14.2, SD-14.3, SD-11.5 |
| SD-14.7 | doc | `docs/features/pdfvt.md` (new): every clause of ISO 16612-2 and -3 with its rule or reading. | `docs/features/pdfvt.md` (new), `docs/README.md` | Every clause is listed with its rule or reading, asserted in `pdfvt_rules.rs`. | SD-14.4, SD-14.5, SD-14.6 |

## SD-15 — PDF/R-1

**PDF/R-1**: the raster-only subset validated and written

**Exit criterion.** as SD-13, from ISO 23504-1

**Doc:** — · **Depends on:** H-102

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-15.1 | build | PDF/R-1 validation from ISO 23504-1: the claim, the file structure the subset allows, one image per page drawn by the fixed content stream, the image formats permitted, and the metadata. | `crates/tinker-pdf/src/pdfr.rs` (new), `crates/tinker-pdf/src/lib.rs`, `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/pdfr.rs` (new) | `pdfr.rs` (new): each rule has a fixture and twin; every clause runs or is a reading by name. | H-102 |
| SD-15.2 | build | PDF/R-1 written: images to a raster-only document through the builder, one per page. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/pdfr.rs` (new), `docs/features/creation.md` | Written files for each permitted image format validate under SD-15.1 with zero findings and pass the strict validator. | SD-15.1 |

## SD-16 — The PDF 2.0 remainder

**The PDF 2.0 remainder**: `/AF` written on any annotation, an image XObject and a document part, and through the editor on an existing document; Table 401's `/DestOutputProfileRef`, `/MixingHints` and `/SpectralData`; attribute namespaces (14.7.6)

**Exit criterion.** each read and written from a cited source, its row in `pdf20-deltas.md` moved; the three unstated semantics wait on the 2.0 text (H-95)

**Doc:** — · **Depends on:** H-95

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-16.1 | build | `/AF` written on every annotation kind the builder and the editor create, and on an image XObject. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/pdf20.rs`, `docs/pdf20-deltas.md` | Each reads back through the existing readers in `pdf20.rs`; the associated-files row drops both from its list of what is left. | — |
| SD-16.2 | build | `/AF` written on a document part. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/pdf20.rs`, `docs/pdf20-deltas.md` | A part's files read back through `associated_files_of`; the row drops document parts. | SD-14.2 |
| SD-16.3 | build | `/AF` through the editor on an existing document: `DocumentEditor` adds and removes associated files on the catalog, a page, an annotation, a structure element and an XObject. | `crates/tinker-pdf-cos/src/edit/mod.rs`, `crates/tinker-pdf/tests/pdf20.rs`, `docs/pdf20-deltas.md` | An edited document reads back with the file on each holder, its saved increment passes the strict validator, and the row drops the editor. | — |
| SD-16.4 | build | Table 401's 2.0 output-intent keys `/DestOutputProfileRef`, `/MixingHints` and `/SpectralData` read and written, from the object-grammar model's keys cited by commit. | `crates/tinker-pdf/src/output_intents.rs`, `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/pdf20.rs`, `docs/pdf20-deltas.md` | Each key written reads back; the page-level output intents row drops them from what is not done. | — |
| SD-16.5 | build | Attribute namespaces (14.7.6): an attribute object's `/NS` read on structure elements and written by `Tag`. | `crates/tinker-pdf/src/structure.rs`, `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/pdf20.rs`, `docs/pdf20-deltas.md` | An attribute in a namespace reads back with it; the namespaced-structure row no longer says attribute namespaces are not read. | SD-04.2 |
| SD-16.6 | build | The three semantics no reachable source states, from the 2.0 text: which namespace a bare-name `/RoleMapNS` value means, whether the global `/RoleMap` applies to a namespaced element, and how a page's output intents combine with the catalog's across subtypes. | `crates/tinker-pdf/src/structure.rs`, `crates/tinker-pdf/src/output_intents.rs`, `crates/tinker-pdf/tests/pdf20.rs`, `docs/pdf20-deltas.md` | Each is implemented from its cited clause with a fixture, and its 'not stated' sentence in `pdf20-deltas.md` is replaced by the clause. | H-95 |

## SD-17 — The 2.0 blend and dash clarifications

**The 2.0 blend and dash clarifications** and the `/Lab` transparency group

**Exit criterion.** each clarification implemented from the 2.0 text with a fixture, each row moving to a status with a doc behind it; `/Lab` groups treated as the owner rules

**Doc:** — · **Depends on:** H-95, H-14

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-17.1 | build | 11.3.5's blend-mode clarifications implemented from the 2.0 text, each with a fixture. | `crates/tinker-pdf-raster/src/blend.rs`, `crates/tinker-pdf/tests/blend_modes.rs`, `docs/pdf20-deltas.md` | Each clarification has a fixture whose pixels are computed in the test from the clause; the blend half of the row moves to a status with a doc behind it. | H-95 |
| SD-17.2 | build | 8.4.3.6's dash clarifications implemented from the 2.0 text, each with a fixture. | `crates/tinker-pdf-raster/src/stroke.rs`, `crates/tinker-pdf/tests/stroke_parameters.rs`, `docs/pdf20-deltas.md` | Each clarification has a fixture; the dash half of the row moves to a status with a doc behind it. | H-95 |
| SD-17.3 | build | A `/Lab` transparency group treated as H-14 rules, the default keeping Lab compositing with a typed warning. | `crates/tinker-pdf-render/src/lib.rs`, `crates/tinker-pdf/tests/blend_modes.rs`, `docs/features/rendering.md` | A fixture with a `/Lab` group renders as the ruling says and carries the warning where the ruling asks for one. | H-14 |

## SD-18 — PDF 2.0, complete

**PDF 2.0, complete**: every 2.0 feature read and written — `/UseBlackPtComp`, `/HTO`, document parts, `/AF` everywhere, 2.0 RichMedia, 3D measurement, projection annotations, `/PtData`, the deprecations — tracked row by row in `pdf20-deltas.md`

**Exit criterion.** `pdf20-deltas.md` lists no feature as tracked; the published 2.0 example files open and validate

**Doc:** — · **Depends on:** H-95, H-22

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-18.1 | doc | Every 2.0 delta entered in `pdf20-deltas.md` as its own row from the 2.0 text: `/UseBlackPtComp`, `/HTO`, document parts, `/AF` holders, 2.0 RichMedia, 3D measurement, projection annotations, `/PtData`, each deprecation, and whatever else the text lists. | `docs/pdf20-deltas.md` | Each delta the text lists has a row naming its clause, its status and the step that owns it. | H-95 |
| SD-18.2 | build | `/UseBlackPtComp` and `/HTO` written by the builder's graphics states; K-01 stores both and RC-02.4 applies the first. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/writer_graphics.rs`, `docs/pdf20-deltas.md` | Each key written reads back through K-01's typed graphics state; both rows move. | K-01.2, K-01.4, RC-02.4, SD-18.1 |
| SD-18.3 | test | Document parts, `/AF` everywhere and `/PtData`, owned by SD-14, SD-16 and AN-10a, checked against the 2.0 text and their rows moved. | `crates/tinker-pdf/tests/pdf20.rs`, `docs/pdf20-deltas.md` | Every holder 2.0 lists for `/AF` is read and written, the hierarchy and `/PtData` match their clauses, and the three rows move. | SD-14.2, SD-16.1, SD-16.2, SD-16.3, AN-10a.1, SD-18.1 |
| SD-18.4 | build | 2.0 RichMedia annotations written: assets, configurations, instances, activation and deactivation, read back through AN-12.3. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/media.rs` (new), `docs/pdf20-deltas.md` | A written RichMedia annotation reads back through AN-12.3's extraction with every part; its row moves. | AN-12.3, SD-18.1 |
| SD-18.5 | build | 3D measurements and projection annotations read and written: a 3D annotation's measurement dictionaries and the `Projection` annotation that carries one. | `crates/tinker-pdf/src/three_d.rs` (new), `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/three_d.rs` (new), `docs/pdf20-deltas.md` | Each measurement kind and a projection annotation written read back with their values; their rows move. | AN-11a.5, AN-02.5, SD-18.1 |
| SD-18.6 | build | The deprecations: each feature 2.0 deprecates read with a typed warning and never written into a document declaring 2.0. | `crates/tinker-pdf-cos/src/write.rs`, `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/tests/pdf20.rs`, `docs/pdf20-deltas.md` | A 2.0 document written by every path carries none of them; reading each gives its warning; the deprecated-features row lists each with its status. | SD-18.1 |
| SD-18.8 | doc | `pdf20-deltas.md` with no feature tracked, and the statements that full 2.0 conformance is not a goal replaced as H-22 records. *Note:* This step carries out what H-22 records: the owner's amendment of `architecture.md`'s locked decisions, as H-08 asks. Until it is made they stand with their pointers at this row (H-08's default), and this step waits on it. | `docs/pdf20-deltas.md`, `docs/architecture.md` | No row's status is tracked; `architecture.md` and the deltas file's own preamble cite H-22's decision. | SD-16, SD-17, SD-18.2, SD-18.3, SD-18.4, SD-18.5, SD-18.6, H-22 |

## SD-19 — PDF 2.0 extensions declared

**PDF 2.0 extensions declared**: each implemented extension (hashes, curves, AES-GCM, integrity, namespace inclusion and MathML in structure) written with its `/Extensions` entry; MathML carried in the structure tree by namespace inclusion

**Exit criterion.** outputs carry the entries; re-read through the extension list

**Doc:** — · **Depends on:** SG-03, SG-04, SE-02, SE-03

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-19.1 | build | One extension-list writer and reader: `/Extensions` entries with prefix, base version, level, URL and revision, a prefix's array form included, the security store's ETSI entry moved onto it, and `Document::extensions` reading them. | `crates/tinker-pdf-cos/src/extensions.rs` (new), `crates/tinker-pdf-cos/src/edit/signing.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/pdf20.rs` | The ETSI entry is written byte-identically to before; each entry shape reads back through `Document::extensions`. | — |
| SD-19.2 | build | ISO/TS 32001's hashes and 32002's curves declared wherever a signature uses them. | `crates/tinker-pdf-cos/src/edit/signing.rs`, `crates/tinker-pdf/tests/pdf20.rs` | A document signed with each declares its entry and re-reads through the extension list; one using neither declares none. | SD-19.1, SG-03, SG-04, H-95 |
| SD-19.3 | build | ISO/TS 32003's AES-GCM and 32004's integrity protection declared wherever a document is written with them. | `crates/tinker-pdf-cos/src/security.rs`, `crates/tinker-pdf/tests/pdf20.rs` | A document written with each declares its entry and re-reads through the extension list. | SD-19.1, SE-02, SE-03, H-95 |
| SD-19.4 | build | ISO/TS 32005's namespace inclusion: MathML carried in the structure tree as elements of the MathML namespace, from a caller's tagged formula and the HTML path's `<math>`, with the entry declared. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf/src/html.rs`, `crates/tinker-pdf/tests/tagged_writer.rs` | A tagged formula's MathML elements read back under the namespace through the structure view, and the entry through the extension list. | SD-19.1, H-95 |

## SD-20 — The object grammar, from the published model

**The object grammar, from the published model**: every object type and key for 1.0–2.0 (type, version introduced and deprecated, required-if, values, direct or indirect) vendored as data from the association's Apache-2.0 model and checked per target version

**Exit criterion.** `validate_grammar(version)` reports unknown, deprecated and missing keys with object paths; each rule class held to fixtures and twins; every corpus rewrite passes at its declared version; K-03's helper calls the grammar check in this row's commit

**Doc:** — · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-20.1 | build | The association's object-grammar model vendored as data under Apache-2.0, its tables for 1.0 to 2.0 read into object types and keys: type, version introduced and deprecated, required-if, possible values, direct or indirect. | `crates/tinker-pdf-cos/data/grammar` (new), `crates/tinker-pdf-cos/src/validate/grammar.rs` (new), `THIRDPARTY.md` | Every type and key of the vendored tables is loaded, counted against the files in a test; `THIRDPARTY.md` records the source, commit and licence; `cargo deny` passes; both builds. | H-92 |
| SD-20.2 | build | The model's predicate expressions evaluated against an object and the target version: required-if, since and deprecated versions, conditional values. | `crates/tinker-pdf-cos/src/validate/grammar.rs` (new) | Each predicate form the tables use has a unit test, and a form the evaluator does not know is a named error counted at zero over the vendored tables. | SD-20.1 |
| SD-20.3 | build | `validate_grammar(version)`: the object graph walked from the trailer, each object typed through the model's links, and unknown, deprecated and missing keys reported with object paths. | `crates/tinker-pdf-cos/src/validate/grammar.rs` (new), `crates/tinker-pdf-cos/src/validate.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/grammar.rs` (new) | `grammar.rs` (new): a fixture and twin for each of the three classes, each finding carrying its object path. | SD-20.2 |
| SD-20.4 | build | Value rules: each key's type, its possible values, and whether it must be direct or indirect. | `crates/tinker-pdf-cos/src/validate/grammar.rs` (new), `crates/tinker-pdf/tests/grammar.rs` (new) | A fixture and twin per rule class. | SD-20.3 |
| SD-20.5 | build | `tpdf check --grammar <version>`. | `tools/tpdf/src/main.rs`, `docs/verification.md` | A CLI test prints the findings the facade gives. | SD-20.4 |
| SD-20.7 | test | K-03's helper calls the grammar check at each written document's declared version. | `crates/tinker-pdf/tests/written_support/mod.rs` (new), `crates/tinker-pdf-cos/tests/written_support/mod.rs` (new) | Every test on the helper holds its output to the grammar, in this step's commit; one injected unknown key fails by name. | K-03.2, SD-20.4 |

## SD-21 — Machine-readable validation reports and feature extraction

**Machine-readable validation reports and feature extraction**: XML, JSON, HTML and text reports with rule, clause, test number and object path; a feature report over actions, annotations, colour spaces, security, embedded files, graphics states, fonts, forms, ICC profiles, images, metadata, outlines, output intents, pages, patterns, shadings and signatures

**Exit criterion.** each format's report re-parsed equals the findings; the feature report of each committed fixture equals its contents walked in the test

**Doc:** — · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-21.1 | build | One report model over every validator's findings, PDF/A, PDF/UA, PDF/X, the strict validator and the grammar: rule, clause, test number and object path, reusing SD-01.3's fields. | `crates/tinker-pdf/src/report.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/reports.rs` (new) | `reports.rs` (new): each validator's findings over committed fixtures convert into the model with no field dropped. | SD-01.3 |
| SD-21.2 | build | XML, JSON, HTML and text serialisations of the model. | `crates/tinker-pdf/src/report.rs` (new), `crates/tinker-pdf/tests/reports.rs` (new) | Each format's report re-parsed in the test equals the findings it was written from; the HTML escapes every value. | SD-21.1 |
| SD-21.3 | build | The feature report's document half: actions, security, embedded files, forms, metadata, outlines, output intents, pages and signatures. | `crates/tinker-pdf/src/feature_report.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/feature_report.rs` (new) | `feature_report.rs` (new): each committed fixture's report equals its contents walked in the test. | — |
| SD-21.4 | build | The feature report's resource half: annotations, colour spaces, graphics states, fonts, ICC profiles, images, patterns and shadings. | `crates/tinker-pdf/src/feature_report.rs` (new), `crates/tinker-pdf/tests/feature_report.rs` (new) | Each committed fixture's report equals its contents walked in the test. | SD-21.3 |
| SD-21.5 | build | `tpdf check --report <format>` and `tpdf features`, the feature report serialised through SD-21.2's formats. | `tools/tpdf/src/main.rs`, `docs/features/document-model.md` | CLI tests print what the facade gives in each format. | SD-21.2, SD-21.4 |

## SD-22 — Policy checking

**Policy checking** over the feature report: institutional rules ("no encryption", "fonts embedded", "image resolution at most N", "no JavaScript", "title present") as a typed rule set

**Exit criterion.** each rule held to fixtures passing and failing it

**Doc:** — · **Depends on:** SD-21

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-22.1 | build | A typed policy rule set over the feature report: no encryption, fonts embedded, image resolution at most N, no JavaScript, title present, and the rest the report can decide, each verdict citing the features it read. | `crates/tinker-pdf/src/policy.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/policy.rs` (new) | `policy.rs` (new): each rule has fixtures passing and failing it. | SD-21.4 |
| SD-22.2 | build | Policies as data a host writes, and `tpdf check --policy`, the verdict in SD-21's report formats. | `crates/tinker-pdf/src/policy.rs` (new), `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/policy.rs` (new) | A policy written as data parses into the typed rules and gives the verdict they give; a malformed policy is a typed error. | SD-22.1, SD-21.2 |

## SD-23 — An XMP model and metadata repair

**An XMP model and metadata repair**: XMP parsed, edited and serialised with schemas and PDF/A extension schemas; `/Info` and XMP kept in step; `pdfaid`, `pdfuaid` and dates repaired

**Exit criterion.** packets round-trip byte-stable when unedited; edits appear in both halves; SD-01's metadata rules pass after repair; `tpdf metadata --repair` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-23.1 | build | An XMP data model: a packet parsed into properties, structures, the three array kinds, qualifiers and `xml:lang` alternatives, and serialised, an unedited packet written back as its original bytes; the PDF/A rule group's own RDF walk (`pdfa/xmp.rs`'s `top_level_properties`) moves onto the model, so one reader serves both. | `crates/tinker-pdf/src/xmp.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/xmp_model.rs` (new), `crates/tinker-pdf/src/pdfa/xmp.rs` | `xmp_model.rs` (new): every committed fixture's packet round-trips byte-stable unedited; a packet edited and re-read gives the edit; every PDF/A fixture's XMP findings are unchanged on the shared reader, and a fault injected in the model's array reading fires in both suites, counted. | — |
| SD-23.2 | build | The predefined schemas typed, Dublin Core, XMP basic, PDF, rights, media management, `pdfaid` and `pdfuaid`, and PDF/A extension schemas read and written. | `crates/tinker-pdf/src/xmp.rs` (new), `crates/tinker-pdf/tests/xmp_model.rs` (new) | Each schema's properties read and write with their types; an extension schema written passes the PDF/A metadata group. | SD-23.1 |
| SD-23.3 | build | Editing through the document editor: properties set, changed and removed, the packet rewritten with padding for later in-place updates. | `crates/tinker-pdf-cos/src/edit/metadata.rs`, `crates/tinker-pdf/src/xmp.rs` (new), `crates/tinker-pdf/tests/xmp_model.rs` (new) | Each edit re-reads through the model after save; the saved increment passes the strict validator. | SD-23.2 |
| SD-23.4 | build | `/Info` and XMP kept in step: an edit to either half appears in both, by the PDF/A mapping of keys to properties with dates converted, in place of `MetadataSync::OtherHalfUnchanged`. | `crates/tinker-pdf-cos/src/edit/metadata.rs`, `crates/tinker-pdf/tests/xmp_model.rs` (new), `docs/features/editing.md` | Each mapped key edited on either side reads back equal on both; an unmapped key leaves the other half alone, asserted; failing first on today's `MetadataSync::OtherHalfUnchanged`. | SD-23.3 |
| SD-23.5 | build | Repair: `pdfaid` and `pdfuaid` made to agree with the claim, dates normalised, the halves reconciled, and the rest of what the PDF/A metadata rules report fixed. | `crates/tinker-pdf/src/xmp.rs` (new), `crates/tinker-pdf-cos/src/edit/metadata.rs`, `crates/tinker-pdf/tests/xmp_model.rs` (new) | Fixtures built to fail each PDF/A metadata rule pass it after repair; a repaired conforming file is unchanged. | SD-23.4 |
| SD-23.6 | build | `tpdf metadata --repair`. | `tools/tpdf/src/main.rs`, `docs/features/editing.md` | The command repairs `testdata/` fixtures to the facade's output. | SD-23.5 |
| SD-23.7 | build | The validators' other walks over an XMP packet move onto SD-23.1's model: `pdfa/xmp.rs`'s `properties` and `packet_header`, `pdfa.rs`'s `pdfaid`, `pdfua.rs`'s `identification` and `pdfa/xmp_extension.rs`'s `read`, each of which builds its own XML source over the packet today, so one reader parses every packet the validators read. | `crates/tinker-pdf/src/xmp.rs` (new), `crates/tinker-pdf/src/pdfa/xmp.rs`, `crates/tinker-pdf/src/pdfa.rs`, `crates/tinker-pdf/src/pdfua.rs`, `crates/tinker-pdf/src/pdfa/xmp_extension.rs` | Every PDF/A and PDF/UA fixture's findings, the extension schemas' included, are unchanged on the model, and a fault injected in the model's property reading fires in the PDF/A, PDF/UA and extension-schema suites, counted; no validator builds its own XML source over a packet. | SD-23.1, SD-01.9, SD-01.10 |

## SD-24a — E-invoices: detection, extraction and creation

**E-invoices: detection, extraction and creation** — the ZUGFeRD and Factur-X profiles and XRechnung detected, the invoice XML extracted, a PDF/A-3 created with the invoice attached with its relationship and the XMP extension schema

**Exit criterion.** created invoices pass SD-01 at 3b and carry the profile the test asked for; detection reads it back

**Doc:** — · **Depends on:** SD-23

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-24a.1 | build | E-invoices detected and extracted: the attached invoice XML found by its file specification and `/AFRelationship`, and the profile, Factur-X's and ZUGFeRD's levels or XRechnung, read from the XMP extension schema as the published Factur-X specification states them, its version cited. | `crates/tinker-pdf/src/einvoice.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/einvoice.rs` (new) | `einvoice.rs` (new): hand-built invoices for each profile are detected with their profile and their XML extracted byte-exact; a document with no invoice reports none. | SD-23 |
| SD-24a.2 | build | E-invoices created: a PDF/A-3 with the invoice XML attached under its relationship and the extension schema written, through `DocumentBuilder::associate_file` and SD-23's model. | `crates/tinker-pdf/src/einvoice.rs` (new), `crates/tinker-pdf/tests/einvoice.rs` (new), `docs/features/creation.md` | Created invoices pass the PDF/A validator at 3b with zero findings, carry the profile each test asked for, and SD-24a.1 reads it back. | SD-24a.1 |

## SD-24b — E-invoices: XML validation

**E-invoices: XML validation** against the published schemas and the EN 16931 business rules

**Exit criterion.** the published example invoices validate, and the rule set's own negative examples fail by rule

**Doc:** `design/einvoice.md` (owed) · **Depends on:** SD-24a

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-24b.1 | design | Create `docs/design/einvoice.md`: the two syntaxes, UN/CEFACT CII and UBL, the XML Schema subset their schemas need, the EN 16931 business rules and how they are evaluated, the CIUS and profile rules, the code lists, the published examples and negative examples as fixtures and their terms, and milestones with exit tests. | `docs/design/einvoice.md` (new) | The doc exists in the design-doc order, names SD-24b.2 to SD-24b.10's exit tests. | SD-24a |
| SD-24b.2 | build | The XML Schema subset the CII and UBL schemas use, in the XML crate's one schema checker, which this step or SG-12.6 creates, whichever lands first: complex types, sequences and choices, occurrence bounds, simple-type facets, namespaces, imports and includes. | `crates/tinker-pdf-xml/src/schema.rs` (new), `crates/tinker-pdf-xml/src/lib.rs`, `crates/tinker-pdf-xml/tests/schema.rs` (new) | `schema.rs` (new): each construct has a document that validates and a twin that fails by name; every construct the two schemas use is covered, counted against them. | SD-24b.1, H-161 |
| SD-24b.3 | build | The CII syntax binding: an invoice read into EN 16931's semantic model of business terms and groups. | `crates/tinker-pdf/src/einvoice/model.rs` (new), `crates/tinker-pdf/src/einvoice/cii.rs` (new), `crates/tinker-pdf/tests/einvoice_validation.rs` (new) | `einvoice_validation.rs` (new): each business term of hand-built CII invoices reads to its value; a term in the wrong place is a typed error with its path. | SD-24b.1, H-161 |
| SD-24b.4 | build | The UBL syntax binding into the same model. | `crates/tinker-pdf/src/einvoice/ubl.rs` (new), `crates/tinker-pdf/tests/einvoice_validation.rs` (new) | A UBL and a CII invoice carrying the same terms read to equal models. | SD-24b.3 |
| SD-24b.5 | build | Both syntaxes validated against the published schemas through SD-24b.2, the schemas vendored with their terms recorded. | `crates/tinker-pdf/src/einvoice/schemas.rs` (new), `crates/tinker-pdf/data/einvoice` (new), `THIRDPARTY.md`, `crates/tinker-pdf/tests/einvoice_validation.rs` (new) | Each published schema loads; a schema-valid invoice passes and its twin with one element out of place fails by path; the terms are in `THIRDPARTY.md`. | SD-24b.2, SD-24b.4 |
| SD-24b.6 | build | EN 16931's core business rules, BR and BR-CO, over the semantic model. | `crates/tinker-pdf/src/einvoice/rules.rs` (new), `crates/tinker-pdf/tests/einvoice_validation.rs` (new) | Each rule has a fixture and twin, its finding naming the rule and the term. | SD-24b.4 |
| SD-24b.7 | build | The VAT-category rules, standard, zero, exempt, reverse charge, intra-community, export and outside scope, with their totals. | `crates/tinker-pdf/src/einvoice/rules.rs` (new), `crates/tinker-pdf/tests/einvoice_validation.rs` (new) | Each rule has a fixture and twin. | SD-24b.6 |
| SD-24b.8 | build | The code-list rules: currencies, countries, units, payment means and the rest the rule set names, the lists carried as data with their terms recorded. | `crates/tinker-pdf/src/einvoice/codes.rs` (new), `crates/tinker-pdf/data/einvoice` (new), `THIRDPARTY.md`, `crates/tinker-pdf/tests/einvoice_validation.rs` (new) | Each list's rule has a fixture and twin; each list's source and terms are in `THIRDPARTY.md`. | SD-24b.6 |
| SD-24b.9 | build | The CIUS and profile rules: XRechnung's rules, and what Factur-X's lower profiles require in place of the full model. | `crates/tinker-pdf/src/einvoice/rules.rs` (new), `crates/tinker-pdf/tests/einvoice_validation.rs` (new) | Each rule has a fixture and twin; an invoice valid at one profile and not another reports both verdicts. | SD-24b.7, SD-24b.8 |
| SD-24b.10 | test | The published example invoices and the rule set's own negative examples run through the validator, and `tpdf invoice --validate`. | `crates/tinker-pdf/tests/einvoice_validation.rs` (new), `corpus/corpora.lock`, `tools/tpdf/src/main.rs`, `docs/features/creation.md` | In CI, where the examples are fetched, every published example validates and every negative example fails by the rule it names; the command prints the verdict. | SD-24b.5, SD-24b.9 |

## SD-25 — PDF Declarations

**PDF Declarations**: XMP declarations of conformance to named profiles, read and written

**Exit criterion.** declarations round-trip

**Doc:** — · **Depends on:** SD-23

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SD-25.1 | build | PDF Declarations read from the XMP model: each declaration's conformance claim, claimant, credentials and report reference, as the PDF Association's published declarations specification states them, cited. | `crates/tinker-pdf/src/xmp.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/declarations.rs` (new) | `declarations.rs` (new): hand-built declarations read back field by field; a malformed one is a typed error. | SD-23.2 |
| SD-25.2 | build | Declarations written through the builder and the document editor. | `crates/tinker-pdf-cos/src/build.rs`, `crates/tinker-pdf-cos/src/edit/metadata.rs`, `crates/tinker-pdf/tests/declarations.rs` (new), `docs/features/document-model.md` | Each declaration written round-trips through SD-25.1's reader, from a new document and from an edited one. | SD-25.1, SD-23.3 |
