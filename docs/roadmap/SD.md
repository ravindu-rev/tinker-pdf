# Capability tier — standards

[The roadmap](../ROADMAP.md) says how a step runs, what comes next, and which dependencies cross tiers. Each row below keeps its text as it was planned — Item, Today, Exit criterion, Doc, Depends on — and is cut into steps, each one workflow run.

Parity and more here means: validate **and convert** to every ISO profile,
check the object grammar against the published model, accessibility from
checking to remediation, PDF 2.0 complete with its extensions, and
e-invoices.

## SD-01 — PDF/A validation, complete

**PDF/A validation, complete**: the 39 staged rules and every machine-checkable clause of parts 1a/1b, 2a/2b/2u, 3a/3b/3u and 4/4e/4f, each finding with clause, test number and object path

**Today.** **39 staged rules** (39 until October 2026, 38 after its rules landed, 39 again since 6.7.2's wider reading was staged). Agreement with the archival validator's conformance corpus is 1 948 of 2 371 PDF/A files — 830 of 831 `pass`, 1 118 of 1 540 `fail` — **measured before the rules below landed, and not re-measured since**: they were written where the corpus is not reachable, each is held to hand-built fixtures and twins, and the next nightly `pdfa_ledger.rs` run owes the agreement figure, the false-positive count (830 of 831 must hold) and the ledger rows each may have made stale. **Landed since, October 2026:** the encoding-CMap clauses (6.3.3.1 and 6.3.3.3, 6.2.11.3.1 and 6.2.11.3.3, 6.2.10.3.x) — a CMap embedded unless the part admits its name (Identity only under part 1, Table 118 under parts 2 to 4), `/WMode` against the program, no reference off the list, an embedded CMap's collection the CIDFont's — leaving a predefined CMap's collection staged under 6.3.3; the XMP packet header's `bytes` and `encoding` attributes (6.7.5, 6.6.2.1, 6.7.2.1) over the catalog's and every page's packet, leaving other metadata streams' headers staged; part 1's prohibition on a filtered metadata stream (6.7.2), over the catalog's — the scope the archival validator's published profile (`070d39f`) gives the rule, where its older wiki statement (`109b482`) reads every metadata stream; that wider reading is the stricter, unmeasured one and is staged by name, which is why the count went back up by one; the content-stream operator list (6.2.10, 6.2.2) — an operator outside ISO 32000's Table A.1, `PS` included, inside `BX`/`EX` or not, in a page, form or appearance stream — leaving the explicitly-associated-resources half staged; an ICC profile's header — version below 3.0 under part 1 and 5.0 after, device class, data colour space — for the destination profile (6.2.2, 6.2.3) and an `ICCBased` space's (6.2.3.2, 6.2.4.2), leaving the profile past its header staged; and 6.2.4.4's two rules, which removed a staged rule outright — every `/Separation` of one name the same alternate and tint transform, compared as objects with direct against indirect and compression ignored, as the clause itself defines the comparison, and every spot colorant of a `/DeviceN` described in its `/Colorants`; font metrics (6.3.6, 6.2.11.5, 6.2.10.5) for a TrueType or CFF program, each drawn code's `/Widths` or `/W` against the advance of the glyph the engine's own mapping selects (`PageResources::selection`, the renderer's and the subsetter's), leaving a Type 1 program and a glyph reached by guess staged; and part 1's transparency prohibition on an XObject's own `/SMask` (6.4, the archival validator's rule 6.4-2), which was neither running nor staged — only a graphics state's soft mask was judged — and may move the Isartor 6.4 ledger row's count of five. **The unagreed `fail` files fall into families**: conformance level A is validated now — `/MarkInfo`, the structure tree root, every structure type after the `/RoleMap`, and every `/Lang` the object graph carries, which took 19 of the 21 logical-structure fixtures and left `830 of 831` where it was; the predefined schemas' value types are read now — the three RDF containers are told apart rather than collapsed to one array, and a property's text is read against the grammar the XMP specification prints for its declared type (`Integer`, `Real`, `Boolean`, `Date`), which took 167 of the 168 files and left `830 of 831` where it was; what stays staged there is the types no fixture exercises (`Rational`, `URI`, `GPSCoordinate` and the rest), since 467 of the 468 `-fail-` fixtures in those two directories are already reported on; then four on an extension schema's custom value types, and six on the packet header's `bytes` and `encoding` attributes — a running rule now — a /Metadata stream that carries a filter (part 1; running now too), and a packet Isartor calls malformed; then 197 in graphics clauses that run and not far enough — the content-stream operator list, the destination profile's header and the Separation tint transform (all running now) — 97 in font clauses waiting on code-to-glyph mapping, content-stream rules as an interpreter rather than a tokenizer (which is what level A's two remaining logical-structure fixtures wait on, and 6.2.11.7.3's five `/ActualText`-per-character ones with them), recursive validation of embedded files, the output intent's own ICC conformance, and 6.11 and 6.12 with fixtures and no rules. Evidence: `PDFA_STAGED` (the `lib.rs` re-export of `STAGED` in `crates/tinker-pdf/src/pdfa.rs`), counted; [features/pdfa.md](../features/pdfa.md); [design/pdfa.md](../design/pdfa.md)

**Exit criterion.** the staged count reaches zero one ledger class at a time; agreement ratcheted up and the false-positive count held at 830 of 831 or better by `pdfa_ledger.rs` nightly

**Doc:** [design/pdfa.md](../design/pdfa.md) · **Depends on:** MS-13

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-02 — PDF/A conversion with fix-ups

**PDF/A conversion with fix-ups**: missing fonts embedded (FT-12), device colour converted or an output intent added (ED-06b, RC-19), transparency flattened for part 1 (FO-10), forbidden actions, annotations and flags removed or repaired, XMP identification and `/Info` synchronised (SD-23), LZW, encryption and external streams removed, non-conforming attachments converted or dropped, `/Lang` added, `/CIDSet` and `/CharSet` fixed, `.notdef` use and `/ToUnicode` repaired; a report of fixed and unfixable

**Today.** a non-goal of the PDF/A design until 9 October 2026 (`design/pdfa.md:67-71`); the archival profile applies only when creating

**Exit criterion.** converted corpus files pass SD-01's validator at the requested level; each fix-up proven on a fixture built to need it; unfixable cases reported by name

**Doc:** `design/pdfa-conversion.md` (owed) · **Depends on:** SD-01, FT-12, ED-06b, FO-10, SD-23

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-03 — An archival profile on rewrite

**An archival profile on rewrite** — the rewrite path can refuse

**Today.** `rewrite` returns `Vec<u8>` with nowhere to put a refusal; the builder has a profile and the rewriter does not ([design/pdfa.md](../design/pdfa.md)); a decision item of the roadmap until 9 October 2026

**Exit criterion.** a rewrite under a profile refuses by clause what the builder refuses; outputs pass SD-01

**Doc:** — · **Depends on:** H-19 (the API change)

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-04 — PDF/UA validation, complete

**PDF/UA validation, complete**: the table grid (milestone 3's other half), part 2's annotations and 7.18's residue (milestone 4), real content (5), UA-2 specifics (6), the ledger and the writer's claim (7), the font clauses that need the glyph list as data; WTPDF 1.0's accessibility and reuse conformance

**Today.** **The validator exists; milestones 1 and 2 of the design landed in October 2026.** `Document::validate_pdfua` and `tpdf check --pdfua` give typed findings numbered by the claimed part and, in the same verdict, every clause not decided as a named `Staged` or `Undecidable` gap (`PDFUA_STAGED`, `PDFUA_UNDECIDABLE`). Milestone 1 moved the census's nine rules in; milestone 2 added twenty-four, among them **the PDF/A font group run for a PDF/UA claim** and re-numbered (7.21.4.1, 7.21.3.2, 7.21.6, 7.21.7 and their 8.4.5 twins), the encoding-CMap rules (7.21.3.1, 7.21.3.3), forbidden `/ToUnicode` values, `/Lang` with the empty-string flag, structure types and `/P`, the metadata stream, `dc:title`, `DisplayDocTitle`, optional content, embedded files, reference XObjects, the accessibility permission and XFA — each held to a fixture and a twin in `pdfua_rules.rs`. **No figure through the facade is measured**: the corpus was not reachable where either milestone landed, so the last figure is still the one from before the facade — 29 of 239 non-conforming fixtures caught, 210 abstained, 0 false alarms over 195 conforming, 16 September 2026 — and the next nightly `corpus.yml` run of `pdfua.rs` owes the caught count, the 7.21 and 8.4.5 abstentions and, first, the false-alarm count over the 195 conforming files (it now finds `corpus/files` without `TINKER_CORPUS`, which it did not, so it had been a nightly skip). **Milestone 3's tree half landed after them**, needing nothing the structure reader did not hold: the content models of tables, lists and TOCs (UA-1 7.2), one `H` per node and no `H` beside an `Hn` (7.4.4), every `Note` with an `/ID` of its own (7.9), a `Formula`'s alternative (7.7) and part 1's empty-`/Alt` reading — each held to a fixture and twin, and owed the same census measurement. **So did milestone 4's part 1 half**: 7.18's annotations against the tree's `/OBJR` kids — in `Annot`, `Form` and `Link` elements, described, `/Tabs /S`, no `TrapNet`, `PrinterMark` an artifact — which reports this engine's own tagged output for a link with no `/Contents` and a page with no `/Tabs` (named in `pdfua.rs` as the tagged writer's to close). Left: the table grid (milestone 3's other half: intersecting cells, rows and columns across spans, header association), part 2's annotations and 7.18's residue (`Form` roles, media clips, `Popup`; milestone 4), real content (5), the UA-2 specifics (6), the ledger and the writer's claim (7), and the font clauses that need the published glyph list (AGL) as data, a glyph's presence in the program or `.notdef`

**Exit criterion.** a rule group that decides the decidable clauses and abstains by name on the rest: the next nightly `pdfua.rs` records caught, abstained and false-alarm counts — the false alarms first, held at zero over the 195 conforming files; each new rule a fixture and twin — schedulable as M halves, one per milestone left

**Doc:** [design/pdfua.md](../design/pdfua.md) · **Depends on:** MS-13, H-41 (the writer's claim)

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-05 — An accessibility checker that lists what a person must check

**An accessibility checker that lists what a person must check**: every human-only condition named with page and element pointers instead of passing it; the failure-condition catalogue and WCAG 2.x success criteria mapped

**Today.** the validator abstains by name on the undecidable clauses (`PDFUA_UNDECIDABLE`)

**Exit criterion.** every catalogue condition mapped to a rule, an abstention or a named person's check; the report lists each instance in fixtures built to raise them

**Doc:** — · **Depends on:** SD-04

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-06 — Colour contrast measured by the renderer

**Colour contrast measured by the renderer**: text against its rendered background, per run, against WCAG's 4.5:1 and 3:1 thresholds

**Today.** absent — the owner's parity decision of 9 October 2026; "contrast" appears only in blend-mode and codec comments; nothing measures text contrast

**Exit criterion.** contrast ratios computed in the test from the fixtures' colours equal the reported ones

**Doc:** — · **Depends on:** SD-05

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-07 — Auto-tagging untagged documents

**Auto-tagging untagged documents**: a structure tree built from the inferred reading order, tables, headings, lists, figures and notes; running heads and feet as artifacts; parent tree, role map, `/Lang` per span; alternate-text placeholders flagged for a person; `/Tabs /S`; annotation references; an outline from headings

**Today.** an untagged file is reported as untagged; auto-tagging a non-goal until 9 October 2026 (`design/pdfua.md:58-63`; `design/tagged-pdf.md:48-51`)

**Exit criterion.** committed tagged documents with their trees removed and auto-tagged score against their own trees above the floors the design states; outputs pass SD-04's machine-checkable rules

**Doc:** `design/auto-tagging.md` (owed) · **Depends on:** FT-30, FT-31

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-08 — Structure-tree editing

**Structure-tree editing**: elements added, removed, moved and re-roled; `/Alt`, `/ActualText`, `/E`, `/Lang` and `/T` set; content made artifact and back; marked-content identifiers bound; table headers, scopes and IDs; list attributes; namespaces; the parent tree kept consistent

**Today.** the tree is read; written by the builder and the EPUB path only

**Exit criterion.** each edit re-read through the structure view; SD-04's rules pass after every edit on the committed tagged fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-09 — An accessibility tree and a screen-reader preview

**An accessibility tree and a screen-reader preview**: roles, names, states, order and boxes from tags or inference, platform-neutral for hosts, and a textual preview

**Today.** assistive-technology output a non-goal until 9 October 2026 (`design/pdfua.md:73-77`)

**Exit criterion.** the tree of each committed tagged document lists its elements in order with the expected roles

**Doc:** — · **Depends on:** SD-07

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-10 — Accessible output by construction

**Accessible output by construction**: the builder, HTML and Markdown paths emit tagged PDF/UA-1 and UA-2 by default, and a conformance-enforcing writer refuses what would break the claim

**Today.** tagged EPUB output and `PageBuilder::tagged`

**Exit criterion.** every builder, HTML and Markdown fixture's output passes SD-04's machine-checkable rules; the claim is stamped only as H-41 decides

**Doc:** — · **Depends on:** SD-04, H-41

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-11 — PDF/X validation and writing, complete

**PDF/X validation and writing, complete**: the 2003 levels measured; the writer's PDF/X flavour (trim and bleed boxes on the builder, output intents); X-4, X-4p, X-5g/n/pg and X-6/6n/6p; X-1a:2001 and X-3:2002; 19005-2's multiple-intent question; the colour registry looked up from bytes the host supplies

**Today.** **The validator exists for the 2003 levels; the writer's flavour does not, and nothing is measured** (October 2026). `Document::validate_pdfx` and `tpdf check --pdfx` read the claim from `/Info` with no XML parse, identify six levels, and run rules under PDF/X-1a:2003 and PDF/X-3:2003 only — encryption, LZW and JBIG2, `/Trapped`, the page boxes, annotations against them, private `/Info` keys, the `GTS_PDFX` intent's shape, `DeviceRGB` under a CMYK profile, PDF/X-3's profile for device-independent colour, transparency, PostScript, embedding — each transcribed from the CGATS application notes v4, the one free restatement of those levels, and held to a fixture and its twin in `pdfx_rules.rs`; every clause the notes do not restate, and every PDF/X-4 clause by its 15930-7 title, is a named `Unread` gap in the verdict (`features/pdfx.md:140`); X-1:2001, X-2 and X-5 were non-goals until 9 October 2026 (`features/pdfx.md:142`). **There is a false-positive bar and no false-negative bar**: no annotated PDF/X corpus exists, so nothing a third party published says a rule fires when it should. And the false-positive bar is empty for these two levels today — the evidence the design names claims none of them: Altona 1.2 is a PDF/X-3:2002 suite, Ghent 5.0 a PDF/X-4 one, and the real claims' readable version strings are `PDF/X-4`, `PDF/X-3:2002`, `PDF/X-1:2001` and empty. `pdfx_census.rs` reads every fetched file nightly and asserts silence on every file that claims nothing; it was written where the corpora are not reachable, so the flavour of each real claim is owed by its first run. Left: the suites pinned (milestone 2, blocked on the Ghent package's licence, H-93) and a third party's file that claims a 2003 level (H-107); the writer's flavour (milestone 6: `ArchivalProfile` is PDF/A's and lives in `tinker-pdf-cos`, `PageBuilder` has no `set_trim_box`, and its shape comparison needs a pinned suite file); 19005-2's multiple-intent question; X-4 and X-6, whose standards' text is H-97 ([features/pdfx.md](../features/pdfx.md))

**Exit criterion.** `pdfx_census.rs` silent on every non-claiming corpus file; an ISO 15930 rule group per level held to fixtures and twins and to a third party's file per level where one exists; the archival profile grows a PDF/X flavour and the writer's output validates at its claimed level

**Doc:** [design/pdfx.md](../design/pdfx.md) · **Depends on:** MS-13, H-93, H-97, H-101

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-12 — PDF/X conversion with fix-ups

**PDF/X conversion with fix-ups**: colour to the output intent, fonts embedded, boxes set, transparency flattened for X-1a and X-3, forbidden content removed

**Today.** a non-goal until 9 October 2026 (`features/pdfx.md:145`)

**Exit criterion.** converted fixtures pass SD-11 at the requested level

**Doc:** [design/pdfx.md](../design/pdfx.md) · **Depends on:** SD-11, ED-06b, FO-10, FT-12

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-13 — PDF/E-1

**PDF/E-1** validation

**Today.** absent — the owner's parity decision of 9 October 2026; a decision row of the roadmap until 9 October 2026, and "out entirely" in the PDF/A design (`design/pdfa.md:64-66`)

**Exit criterion.** each rule held to a fixture and twin transcribed from ISO 24517-1

**Doc:** — · **Depends on:** H-102

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-14 — PDF/VT-1, 2 and 3

**PDF/VT-1, 2 and 3**: the document-part hierarchy (`/DPartRoot`, `/DParts`, record metadata) validated and written

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'DPartRoot|DParts' crates tools` finds nothing

**Exit criterion.** as SD-13, from ISO 16612-2/3

**Doc:** — · **Depends on:** H-102

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-15 — PDF/R-1

**PDF/R-1**: the raster-only subset validated and written

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '23504' crates tools` finds nothing

**Exit criterion.** as SD-13, from ISO 23504-1

**Doc:** — · **Depends on:** H-102

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-16 — The PDF 2.0 remainder

**The PDF 2.0 remainder**: `/AF` written on any annotation, an image XObject and a document part, and through the editor on an existing document; Table 401's `/DestOutputProfileRef`, `/MixingHints` and `/SpectralData`; attribute namespaces (14.7.6)

**Today.** **Associated files, page-level output intents and namespaced structure types landed**, each read and written, each with its row in [pdf20-deltas.md](../pdf20-deltas.md) and implemented only from what a reachable source states (the approved errata at `b25fc23`, the association's object-grammar model at `c48b363`, the archival validator's published rules at `109b482`, the association's 2.0 examples at `c20f2c1`): namespaces (`add_namespace`, `Tag::namespace`, `map_role_in`; `StructElement::namespace`/`standard_namespace`); page-level output intents (`PageBuilder::output_intent`; `Page::output_intents` beside `Document::output_intents`); associated files on the catalog, a page, a structure element, a link annotation, a form XObject and the structure tree root, and 14.13.5's marked-content form the errata add as Table 409a — `pdf20.rs`'s four tests from `associated_files_are_written_on_an_annotation_a_form_and_the_structure_root_and_read_back` to `the_annotation_and_marked_content_listings_spend_one_budget`. **What is left, by name:** `/AF` *written* on an annotation the builder does not make, an image XObject or a document part, and through the editor on an existing document (each is read); Table 401's 2.0 output-intent keys `/DestOutputProfileRef`, `/MixingHints`, `/SpectralData`; attribute namespaces (14.7.6); an `EncryptedPayload` written is SE-05's. Not stated in any reachable source, and so reported rather than guessed: which namespace a bare-name `/RoleMapNS` value means, whether the global `/RoleMap` is withheld from a namespaced element, and how a page's output intents combine with the catalog's across subtypes

**Exit criterion.** each read and written from a cited source, its row in `pdf20-deltas.md` moved; the three unstated semantics wait on the 2.0 text (H-95)

**Doc:** — · **Depends on:** H-95

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-17 — The 2.0 blend and dash clarifications

**The 2.0 blend and dash clarifications** and the `/Lab` transparency group

**Today.** still "tracked" in [pdf20-deltas.md](../pdf20-deltas.md). **The blend and dash half cannot be read here**: on 26 September 2026 the sponsored ISO 32000-2 copy at pdfa.org was refused by the egress policy (403), and the two reachable PDF Association sources — the `pdf-issues` errata and the object-grammar model — carry no 8.4.3.6 erratum and nothing about 11.3.5 beyond a NOTE on its figures; pdf20-deltas.md lists what each said. The errata *do* quote Table 145's `/CS` text excluding Lab from a transparency group's colour space, so the `/Lab` group [features/rendering.md](../features/rendering.md) composites in Lab is a file the specification does not permit — what to do with one is a decision, not a delta (H-14)

**Exit criterion.** each clarification implemented from the 2.0 text with a fixture, each row moving to a status with a doc behind it; `/Lab` groups treated as the owner rules

**Doc:** — · **Depends on:** H-95, H-14

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-18 — PDF 2.0, complete

**PDF 2.0, complete**: every 2.0 feature read and written — `/UseBlackPtComp`, `/HTO`, document parts, `/AF` everywhere, 2.0 RichMedia, 3D measurement, projection annotations, `/PtData`, the deprecations — tracked row by row in `pdf20-deltas.md`

**Today.** "full 2.0 conformance is not a current goal" (`architecture.md:26-31`)

**Exit criterion.** `pdf20-deltas.md` lists no feature as tracked; the published 2.0 example files open and validate

**Doc:** — · **Depends on:** H-95, H-22

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-19 — PDF 2.0 extensions declared

**PDF 2.0 extensions declared**: each implemented extension (hashes, curves, AES-GCM, integrity, namespace inclusion and MathML in structure) written with its `/Extensions` entry; MathML carried in the structure tree by namespace inclusion

**Today.** absent — the owner's parity decision of 9 October 2026; `/Extensions` is written only to declare the security store's ETSI extension (`crates/tinker-pdf-cos/src/edit/signing.rs:296-313`)

**Exit criterion.** outputs carry the entries; re-read through the extension list

**Doc:** — · **Depends on:** SG-03, SG-04, SE-02, SE-03

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-20 — The object grammar, from the published model

**The object grammar, from the published model**: every object type and key for 1.0–2.0 (type, version introduced and deprecated, required-if, values, direct or indirect) vendored as data from the association's Apache-2.0 model and checked per target version

**Today.** the strict validator is hand-written; the model consulted for 2.0 deltas at `c48b363` (SD-16)

**Exit criterion.** `validate_grammar(version)` reports unknown, deprecated and missing keys with object paths; each rule class held to fixtures and twins; every corpus rewrite passes at its declared version; K-03's helper calls the grammar check in this row's commit

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-21 — Machine-readable validation reports and feature extraction

**Machine-readable validation reports and feature extraction**: XML, JSON, HTML and text reports with rule, clause, test number and object path; a feature report over actions, annotations, colour spaces, security, embedded files, graphics states, fonts, forms, ICC profiles, images, metadata, outlines, output intents, pages, patterns, shadings and signatures

**Today.** typed findings and `tpdf check`

**Exit criterion.** each format's report re-parsed equals the findings; the feature report of each committed fixture equals its contents walked in the test

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-22 — Policy checking

**Policy checking** over the feature report: institutional rules ("no encryption", "fonts embedded", "image resolution at most N", "no JavaScript", "title present") as a typed rule set

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'policy_check|PolicyRule' crates tools` finds nothing

**Exit criterion.** each rule held to fixtures passing and failing it

**Doc:** — · **Depends on:** SD-21

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-23 — An XMP model and metadata repair

**An XMP model and metadata repair**: XMP parsed, edited and serialised with schemas and PDF/A extension schemas; `/Info` and XMP kept in step; `pdfaid`, `pdfuaid` and dates repaired

**Today.** XMP read as raw bytes and written verbatim; `MetadataSync::OtherHalfUnchanged` (`features/editing.md:849`)

**Exit criterion.** packets round-trip byte-stable when unedited; edits appear in both halves; SD-01's metadata rules pass after repair; `tpdf metadata --repair` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-24a — E-invoices: detection, extraction and creation

**E-invoices: detection, extraction and creation** — the ZUGFeRD and Factur-X profiles and XRechnung detected, the invoice XML extracted, a PDF/A-3 created with the invoice attached with its relationship and the XMP extension schema

**Today.** associated files and PDF/A-3 writing exist

**Exit criterion.** created invoices pass SD-01 at 3b and carry the profile the test asked for; detection reads it back

**Doc:** — · **Depends on:** SD-23

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-24b — E-invoices: XML validation

**E-invoices: XML validation** against the published schemas and the EN 16931 business rules

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'en.?16931|xrechnung|zugferd|factur-x' crates tools` finds nothing

**Exit criterion.** the published example invoices validate, and the rule set's own negative examples fail by rule

**Doc:** `design/einvoice.md` (owed) · **Depends on:** SD-24a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SD-25 — PDF Declarations

**PDF Declarations**: XMP declarations of conformance to named profiles, read and written

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'pdfd:' crates tools` finds nothing

**Exit criterion.** declarations round-trip

**Doc:** — · **Depends on:** SD-23

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*
