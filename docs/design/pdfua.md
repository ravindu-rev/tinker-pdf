# PDF/UA validation

`Document::validate_pdfua` answers "does this file conform to ISO 14289-1
(PDF/UA-1) or ISO 14289-2 (PDF/UA-2), and which clause did it break?" with the
typed findings and the stated coverage `validate_pdfa` gives, for exactly the
clauses a reader can decide — and it **abstains by name** on the rest, because
most of ISO 14289 is a judgement about meaning that no reader can make. The
veraPDF corpus's PDF/UA fixtures are its census, in
`crates/tinker-pdf/tests/pdfua.rs`.

**Much of it is shared with PDF/A**: level A validates the logical structure,
`/StructTreeRoot`, the role map against 14.8.4's standard types and every
`/Lang` the object graph carries; the PDF/A font group answers most of what
14289's font clauses ask; and the annotation model, the XMP parser and the
structure-tree reader all exist. This document says what is shared and what
is new.

## Scope

- **Both parts.** ISO 14289-1:2014, whose corpus clauses are 5 and 7.1 to
  7.21, and ISO 14289-2:2024, whose corpus clauses are 5 and 8.2 to 8.14 and
  which is defined on ISO 32000-2.
- **Flavour detection**: `pdfuaid:part` from the XMP packet, and for UA-2
  `pdfuaid:rev` as a four-digit year — read by `pdfua.rs`'s own reader, kept
  separate from `pdfa/xmp.rs`'s, because reading one identification schema
  through the other reads PDF/UA files as PDF/A claims.
- **The bar**: the veraPDF corpus's PDF/UA fixtures, whose expected verdict
  is in the filename and whose expected *message* is in the document outline.
  Every fixture but a few `-pass-` twins states its own expectation there
  (read with this repository's own `tpdf outline`), and the outlines are the
  requirement source wherever the clause text alone would not give the rule;
  several overturn a reading the clause text invites. Data, admissible under
  ruling 13 ([rulings.md](../rulings.md)); the tool that produced them is not
  invoked.
- **Rule groups by machinery**, as `pdfa/` is organised: syntax, metadata,
  structure, fonts, annotations, and one group that is new to this tree —
  **real content**, over the recording device.
- **Abstention as a typed answer.** A verdict says which groups ran
  (`Coverage`) *and* which clauses this build knows it cannot decide, in two
  classes: staged (machinery missing, could land) and undecidable (a judgement
  about meaning, will never land by a reader alone).
- **Surfacing**: `Document::validate_pdfua`, and `--pdfua` on `tpdf check`.

## Non-goals

- **Judging meaning.** Whether a `/P` is a paragraph, whether the reading
  order is the author's, whether an `/Alt` describes the picture, whether a
  heading is a heading, whether colour contrast suffices. These are not
  implemented and never will be by a reader alone. Every such clause is an
  `undecidable` entry with its reason, printed as abstention and never counted
  as agreement. Colour contrast measured as a ratio by the renderer is
  [ROADMAP](../ROADMAP.md) row SD-06; whether a ratio suffices for a reader,
  and every other clause here, stays a person's.
- **Auto-tagging.** No structure is inferred for an untagged file; it is
  reported as untagged. [design/reading-order.md](reading-order.md) infers an
  order and labels it inferred, and nothing it infers reaches a verdict here.
  Auto-tagging into a new document is [ROADMAP](../ROADMAP.md) row SD-07; an
  inferred tree never reaches a verdict about the file it was inferred from.
- **Fixing a file.** No repair mode; validation reports and the tagged writer
  (`PageBuilder::tagged`) conforms or refuses, as the PDF/A profile does.
  Structure-tree editing and repair are [ROADMAP](../ROADMAP.md) rows SD-07
  and SD-08.
- **A full PDF 2.0 namespace model.** ISO 14289-2's role maps are namespaced
  (`/RoleMapNS`, `/NS`). The structure reader follows them
  ([pdf20-deltas.md](../pdf20-deltas.md)); UA-2's rules stated over them are
  staged, not designed around here.
- **Assistive-technology output.** Bytes to verdicts; a screen-reader bridge
  is an embedder's. A platform-neutral accessibility tree and a textual
  preview are [ROADMAP](../ROADMAP.md) row SD-09; the bridge itself stays the
  embedder's.

## What is shared, and what is new

Every fixture falls into one of four columns: a rule ISO 19005 already has,
run under a second clause table; a rule that is this validator's own; a rule
staged behind machinery; and a judgement no reader makes.

### Shared: PDF/A rules under ISO 14289's numbering

| Requirement, as the fixtures state it | Where it lives | Fixtures |
| --- | --- | --- |
| `/StructTreeRoot` present; `/MarkInfo /Marked true`; `/Suspects` not true | `pdfua/structure.rs`, with the PDF/A finding kinds | 7.1-t04, 7.1-t11, 8.2.1 |
| Every structure type resolves through the `/RoleMap` to a standard type, case-sensitively; no remap to the empty string; two non-standard types mapped to each other; `Document` remapped to `Book` | the level A structure-type rule in `pdfa/logical.rs`, over `crate::structure::bind`, numbered UA-1 7.1 and UA-2 8.2.1; a standard type remapped is read off what the structure reader resolved, so `/P /P` is not a remap; UA-2's 8.2.4, stated over namespaces, is staged | 7.1-t05 to t07, 8.2.4-t01, t02, t04 |
| `/Lang` well formed: primary subtag 1 to 8 letters (`portugue` passes, `portugues` fails), a digit primary fails, a Cyrillic subtag fails, `1234abcd` passes and `1234abcde` fails | `pdfa::logical::language_is_well_formed` under `LanguageGrammar::PDF_UA`, the RFC 3066 form | 7.2-t29, 8.4.4-t02 |
| Fonts: embedding (7.21.4.1), `/CIDToGIDMap` (7.21.3.2), a TrueType font's `/Encoding` (7.21.6), `ToUnicode` with its exemptions (7.21.7-t01), widths against the program (7.21.5), the encoding CMap (7.21.3.3) and the CIDFont's collection (7.21.3.1) | `pdfa::fonts::run` under a part 2, level U reading, re-numbered by `pdfua::fonts::ua_clause_of` | 7.21 and 8.4.5 |
| Optional content configuration: `/D` carries a non-empty `/Name`; no `/AS` | `pdfua/syntax.rs`; ISO 19005-2 6.9 asks the same | 7.10, 8.7 |
| Embedded file specification carries `/F` and `/UF` (UA-1), a `/Desc` in `/EmbeddedFiles` (UA-2) | `pdfua/syntax.rs` | 7.11, 8.14 |
| A metadata stream exists; `dc:title` present; `/ViewerPreferences /DisplayDocTitle true` | `pdfua/syntax.rs`, and `dc:title` read with the claim in `pdfua.rs` | 7.1-t08 to t10, 8.11 |

**One shared grammar, split by one flag.** The level A group admits
`/Lang ()` because `6-8-4-t01-pass-d` says the empty string "is permitted";
`7.2-t29-fail-n`, `-fail-o` and `-fail-p` say the opposite for PDF/UA —
"Value of Lang is empty" is annotated `fail` in the catalog, on a `/P` and in
a marked-content sequence. Same function, one parameter, two standards; and
the corpus is what says so, not either clause.

### Fixture-decided details the clause text would not give

- **An empty `/ActualText` stands and an empty `/Alt` does not** on a
  `Figure` (7.3-t01-pass-c against -fail-b) and on a `Formula`
  (7.7-t01-pass-c against -fail-b) under part 1; under part 2, whose
  published condition is `Alt != null`, an empty `/Alt` stands.
- **An annotation with the hidden flag, or whose `/Rect` lies outside the
  page, needs no `/Contents` and no `/Alt`** (7.18.1-t02-pass-c, -pass-d); a
  widget's `/TU` is read from the **field**: a field carrying one passes
  (7.18.1-t03-pass-e) and a field with none whose two widget kids each carry
  one fails (-fail-d).
- **A media clip's `/CT` may be empty and still pass**; its `/Alt` may not
  (7.18.6.2-t01-pass-a, -t02-fail-b). The rule is staged.
- **`FREETEXT` in upper case is "not standard"** and the fixtures that carry
  it are annotated `pass` — the annotation census in
  `crates/tinker-pdf/tests/annotation_census.rs` names such subtypes as
  `AnnotationKind::Other`, and the annotation group does not judge them.
- **A `List` may contain a `List`** (7.2-t17-pass-g) while "L is present in
  List item" fails (7.2-t20-fail-b), as the published conditions give: 7.2-19
  admits `L` in `L`, and 7.2-20 admits only `Lbl` and `LBody` in `LI`.

### New: decided from the tree

`pdfua/structure.rs` decides the structure grammar from what the structure
reader holds — an element's standard type, its parent's and its kids': the
content models of tables, lists and TOCs (veraPDF's 7.2-3 to 7.2-20, 7.2-26
to 7.2-28 and 7.2-36 to 7.2-40), headings numbered without skips and
starting at H1 in reading order (7.4.2), 7.4.4's one `H` per node and no `H`
beside an `Hn`, 7.9's `Note` identifiers present and unique, and 7.7's
`Formula`. Two readings, both towards silence: a kid whose type resolves to
nothing standard is not judged here, since the structure-type rule already
reports it; and the structure tree root, as a parent, is no standard type.

| Requirement | What it needs |
| --- | --- |
| Table regularity — cells that intersect, rows and columns that disagree across `RowSpan`/`ColSpan` (7.2-t41 to t43, veraPDF's 7.2-15; 8.2.5.26-t03, t04); header association — every `TD` reachable through a `Scope` on a `TH` or through `/Headers` naming an `/ID` (7.5, 8.2.5.26-t05, t06) | **the table's grid.** The structure reader reads `/ID` and the `/Table` owner's `/A` attributes (`/Headers`, `/Scope`, `/Summary`, `/RowSpan`, `/ColSpan`); the grid over them is the one [design/table-reconstruction.md](table-reconstruction.md) scores against, built once. Staged |
| UA-2's grammar: `H` forbidden (8.2.5.12), `Math` inside `Formula` (8.2.5.29), a `Document` element at the root (8.2.5.2), `ListNumbering` not `None` (8.2.5.25), `Link` elements whose annotations target different destinations (8.2.5.20) | the structure grammar ISO 32000-2 and ISO/TS 32005 define. Staged under 8.2 |
| Namespaced role maps and the PDF 2.0 namespace on `Document` (8.2.4-t02-c, -t03, 8.2.5.2-t02) | the namespaced role maps the reader follows. Staged under 8.2 |

### New: annotations in the structure tree

`pdfua/annotations.rs` reads each page's `/Annots` against the element whose
`/OBJR` kid names the annotation — not through `/StructParent` and the
`/ParentTree`, as veraPDF does; the two agree on a well-formed file, and where
an `/OBJR` has no `/StructParent` back to it this reading finds an enclosing
element veraPDF does not, which errs towards silence. Under part 1 (7.18):
every annotation an `/OBJR` kid of an element; `Link` annotations inside a
`Link` element, widgets inside `Form`; `/Contents` or `/Alt` with the
exemptions above; `Tabs /S` on every page with annotations; `TrapNet`
forbidden; `PrinterMark` in no element. With no tree at all the enclosing
rules do not run, since the missing tree is already the finding. Staged: a
`Form` element without a `Role` holding one widget (7.18.4-2 — the structure
reader does not read `PrintField` attributes), a media clip's `/CT` and `/Alt`
(7.18.6.2 — it sits behind a `Screen` annotation's rendition action), a
`Popup`'s own tagging, which the published condition does not exempt; and
part 2's annotations (8.9: artifacts when invisible, enclosed in `Annot` or
`Link` elements, described, `/Tabs` on their pages) and widgets (8.10:
enclosed in `Form` elements, one per element).

### New: COS reads

Decided, in `pdfua/syntax.rs`: no reference XObject (7.20-1), the
accessibility permission bit in the encryption `/P` (7.16-1), no XFA under
part 2 (8.10.1-3). Staged: dynamic XFA under part 1 (7.15), which is the XFA
packet's own statement; PUA code points in text strings (8.6); structure
destinations in outlines and `GoTo` actions (8.8).

### New: the real-content group, which needs the interpreter

Staged. These `-fail-` fixtures ask a question no walk over dictionaries
answers. 7.1-t01 to t03 and 8.2.2: *every* piece of content a page draws is
either inside an `/Artifact` marked-content scope or inside a scope carrying
an `/MCID` that some structure element claims; an `/Artifact` inside real
content fails; a page whose content stream never distinguishes an artifact
fails; an image "not marked as Artifact or real content" fails. 7.20-t02: a
form XObject that carries an `/MCID` and is invoked three times fails, invoked
once passes.

`pdfa/content.rs` is a tokenizer by design — it tracks the text rendering
mode and the selected font and nothing else, and a rule that needs more needs
`tinker_pdf_content::interpret` and a `Device`. This is that rule. The seam is
the one ruling 7 makes the only seam, and the device is `RecordingDevice`
(`crates/tinker-pdf-content/src/record.rs`) under `Capture { text: true,
paths: true, images: true, structure: true, state: false }`. For every
`ShowGlyph`, `FillPath`, `StrokePath`, `DrawImage` and `DrawShading` event,
`scopes_at(index)` gives the scopes open at the call, outermost first; a scope
tagged `Artifact` settles the first question, and a scope whose
`MarkedScope::props` carries an `/MCID` is looked up in the set of
`(stream, mcid)` pairs the tree claims — the set `StructureTree::text_for_page`
computes for characters, generalised to every event. `BeginForm { id }`
events counted per `id` settle 7.20-t02.

What the recorder does not give this group: annotation appearance streams are
drawn by the reader (12.5.5) and `Page::text()` interprets the page's stream
and the forms it invokes, never an `/AP` — the content walk in
`pdfa/content.rs` visits appearances and the interpreter does not, so an
appearance stream's real-content question is answered by a second
interpretation per appearance or not at all, and the group says which. And the
recorder records; it does not decide. The join, the claimed set and the two
exemptions the fixtures state (7.1-t01-pass-b: an artifact inside a `/P` that
has no `/MCID` passes; 7.1-t02-pass-b: "junk" inside an artifact with no
`/MCID` passes) are this group's to write.

### Undecidable, and named as such

Whether tagging is *correct*. The conforming fixtures and the non-conforming
ones both contain files whose difference is only visible to a person — a `/P`
around a heading, an `/Alt` that lies. `PDFUA_UNDECIDABLE` is a static list
keyed by clause with the reason, mirrored into the verdict, and the census
prints its members as abstention with the word *undecidable* beside them
rather than the word *staged*. The distinction matters for scheduling: a
staged clause is a milestone; an undecidable one is a limit.

## Design

**Where it lives.** `crates/tinker-pdf/src/pdfua.rs` and `pdfua/`
(`syntax.rs`, `structure.rs`, `annotations.rs`, `fonts.rs`), beside `pdfa/`.
**The kernel shared, by opening it rather than moving it**: `Machinery::new`
and `reaches`, and the UTF-32 transcode `readable`, are `pub(crate)` in
`pdfa.rs`, so the PDF/UA validator counts its reaches with the same
`Machinery`; [design/pdfx.md](pdfx.md) shares it the same way.
`hostile_input.rs` calls the validator twice per input, whole and with the
structure group alone.

**One rule, two standards, two clause tables.** A shared rule keeps its
ISO 19005 numbers and gains a second table keyed by ISO 14289 part — 7.21.4.1
under UA-1, 8.4.5.5.1 under UA-2, for the embedding rule that is 6.3.4 under
19005-1 — and reports under the numbering of the standard whose validator ran
it. The PDF/A font group runs for a PDF/UA claim under a part 2, level U
reading (`pdfa::fonts::run`, split from the group's counted reach so the
PDF/UA group reaches once), and `pdfua::fonts::ua_clause_of` re-numbers what
it finds kind by kind. A kind with no PDF/UA clause is dropped rather than
given the nearest number: the subset tag, the program/key pairing, an
unreadable program, part 1's `/Differences`, and `CidSystemInfoIncomplete` —
7.21.3.1 is a relationship between two dictionaries, which
`CidSystemInfoMismatch` reports, and veraPDF's test passes any CIDFont
collection under `Identity-H`. Embedding is judged over the fonts drawn at a
visible rendering mode, as the PDF/A group judges it and as veraPDF's
7.21.4.1-1 exempts `renderingMode == 3`. What a drawn code selects past its
width — glyph presence, `CharSet` and `CIDSet`, the widths of a Type 1
program, `.notdef`, and a `/Differences` name absent from the Adobe Glyph
List, which is not vendored — is staged under 7.21. The `/Lang` grammar is
`pdfa::logical::LanguageGrammar`, one function and one flag: PDF/UA reads
RFC 3066 and refuses `()`, as 7.2-29's and 8.4.4-2's published pattern does,
and `the_empty_language_is_pdfa_conforming_and_pdfua_not` holds both sides.
Part 2 also asks for the catalog's own `/Lang` (8.4.4-1), which subsumes "a
language stated somewhere", so that rule has no part 2 row. A file claiming
both standards is validated by both, and the two verdicts share the finding
kinds and nothing else. `validate_pdfua` runs its groups whatever the claim,
and numbers a file that claims no part as part 1 does.

**Two rules have no row under ISO 14289-2.** veraPDF's `PDFUA-2.xml` states
neither `/Suspects` nor heading order: PDF 2.0 deprecates `/Suspects`, and the
part forbids `H` at 8.2.5.12 instead of ordering `Hn`. Neither runs under
part 2.

**`FindingKind` stays one closed enum.** A missing `Tabs /S` is a variant of
its own (`TabOrderNotStructure`), as is a `Figure` without `/Alt`, which
ISO 19005 does not ask (14.9.3 is a PDF/UA requirement). Adding a variant is a
reviewable change to what "conforms" means, as it is for PDF/A.

**Coverage grows a second axis.** `Coverage` says which groups ran.
`PdfUaVerdict::abstained` is a `Vec<PdfUaAbstention>`, each a class and a
`PdfUaGap { part, clause, rule, because }`, built from `PDFUA_STAGED` and
`PDFUA_UNDECIDABLE` and filtered to the part claimed (part 1 where none is),
so a caller reading an empty finding list sees, in the same struct, the
clauses nobody decided. `every_part_abstains_on_something_in_both_classes` is
the unit test over the two lists.

**The acceptance criterion.** Over the conforming fixtures, **the false-alarm
count is zero, as a hard assertion** in `pdfua.rs`. The caught count is a
floor recorded from a run, never aimed at. A rule that raises the floor by
reporting a conforming file has raised it for nothing.

**The ledger** is milestone 7 and does not exist:
`crates/tinker-pdf/tests/pdfua_ledger.tsv`, one row per abstained `-fail-`
file, each with a class — `staged`, `undecidable`, `reading` — and a
mandatory reason; a row without a reason is refused by the reader; a row whose
subject no longer abstains fails as stale; an abstention with no row fails as
uncovered — the discipline `pdfa_ledger.tsv` has.

**The independent check outside the suite.** The fetched corpora carry
tagged files real producers wrote, none claiming PDF/UA, so none is scored.
Run through the structure and real-content groups, a finding on a real-world
tagged file is printed with its kind (milestone 5) — the check that says
whether a rule survives contact with what producers actually write into a
role map.

**The writer.** The tagged writer claims nothing about PDF/UA: no `pdfuaid`
packet is written. A `pdfuaid` packet on the archival profile is one field
(milestone 7), and every tagged writer fixture is then judged by this
validator with complete coverage and zero findings — the same asymmetry
pdfa.md records (the writer is checked by the table that would accept its
mistakes), narrowed the same way (twins), and not closed. `pdfua.rs` asserts
the findings this engine's own output carries, each named with its reason.

## Milestones

1. The kernel shared; `pdfua/` with the census's rules moved in; `PdfUaVerdict`
   with abstentions; `Document::validate_pdfua`; `tpdf check --pdfua`;
   `PDFUA_STAGED` and `PDFUA_UNDECIDABLE`.
2. Second clause tables on the shared rules — fonts, `/Lang` with the
   empty-string flag, role map and structure types, optional content, embedded
   files, the three metadata rules — and the font rules with no PDF/A
   analogue: `WMode` agreement between an `/Encoding` CMap stream and its
   dictionary (7.21.3.3-t02), the forbidden `ToUnicode` values `<0000>`,
   `<FFFE>` and `<FEFF>` (7.21.7-t02), a `/Differences` name absent from the
   Adobe Glyph List (7.21.6-t02-d).
3. The attribute reader in `structure.rs` (`/A`, `/ID`, `/Headers`, `/Scope`,
   `/RowSpan`, `/ColSpan`, `/ListNumbering`) and the structure-grammar group:
   tables, lists, TOC, headings both kinds, `Math`, `Document`, `Note` IDs,
   table regularity and header association.
4. The annotations-in-structure group (7.18, 8.9, 8.10, 8.14) over
   `Page::annotations()` and `StructKid::Object`.
5. The real-content group over `RecordingDevice`, and the tagged files of the
   fetched corpora run through it with every finding printed with its kind.
6. UA-2 specifics: `pdfuaid:rev`, PUA, structure destinations, `Link`
   targets, `AFRelationship`, and the namespaced role-map rules.
7. The ledger, `pdfua_ledger.tsv`, with uncovered and stale assertions;
   `pdfuaid` on the archival profile; every `PageBuilder::tagged` fixture in
   `pdfa_writer.rs` validating under PDF/UA-1 with zero findings and its
   untagged twin not.

## Dependencies

- **`crates/tinker-pdf/src/pdfa/`** — the kernel and the font, annotation
  and XMP machinery, shared by opening items to the crate rather than copied.
- **`crates/tinker-pdf/src/structure.rs`** — `bind`, the role-map
  resolution, `text_for_page`'s claimed set, `/ID` and the `/Table` owner's
  attributes, under the reader's bounds (`MAX_STRUCTURE_DEPTH`, the element
  cap, `MAX_STRUCTURE_VALUES`). It reads no `/ListNumbering` and no
  `PrintField` attributes.
- **`crates/tinker-pdf-content/src/record.rs`** — the recording device. The
  real-content group needs `structure: true`; `Capture::GLYPHS` records no
  scope.
- **`Page::annotations()`** — the 28-subtype model with `/F`, `/Contents`
  and `/Popup`; the per-family payloads are not needed here.
- **PDF 2.0 namespaced structure types** — read and written
  ([pdf20-deltas.md](../pdf20-deltas.md)); the UA-2 rules over them are
  staged.
- **The published PDF/UA profiles**, read as data: veraPDF's `PDFUA-1.xml`
  ("Validation rules against ISO 14289-1:2014") and `PDFUA-2.xml` (against
  ISO 14289-2:2024) enumerate, per clause, what their authors found
  decidable. They are the same standing the machine-readable PDF/A profiles
  have in [design/pdfa.md](pdfa.md) — a published statement about which
  clauses are checkable, admissible to read and never run.
- **The standards' text.** ISO 14289-1 and -2 are sold; the corpus's
  directory names, outlines and the profiles above are the clause map this
  design is written from, and a rule is staged rather than written where
  those three disagree.

## Risks

| Risk | Mitigation |
| --- | --- |
| **A rule that is right by the clause and wrong by the corpus.** PDF/UA's fixtures state several (an empty `/ActualText`, an off-page annotation, an inherited `/TU`, an empty `/CT`, upper-case `FREETEXT`) | The pass twins are the requirement. Every rule lands with the pass fixture that would report it if it were too strict, and the false-alarm assertion is the gate — a rule that catches ten and alarms once does not land |
| **Abstention read as a pass.** A verdict with no findings from a validator that decided four clauses of forty | `PdfUaVerdict::abstained` is in the same struct as `findings`; `tpdf check --pdfua` prints both; the census prints *undecidable* and *staged* as words, never as a rate |
| The attribute walk over `/A` and `/ID` is a bounded recursion on untrusted input | The `trees.rs` discipline the structure reader uses — depth cap, element cap, the walk-wide value and byte budgets, typed truncation warnings — and `hostile_input.rs` calls `validate_pdfua` |
| The corpus is atomic and one-clause; a rule tuned to it reports real documents | The fetched corpora's real tagged files run through the groups, every finding printed with its kind (milestone 5); a kind that fires on dozens of real files is a rule to re-read, whatever the fixtures say |
| The pass fixtures with no outline have no stated reason to pass | They are annotated by name and their reason is in their `-fail-` twin's outline; a rule that reports one has no message to argue with and is wrong on the annotation alone |
| Two standards sharing one rule drift when one is edited for a fixture the other lacks | The empty-`/Lang` flag is the pattern: the split is a parameter with a test on each side, never a second function |
| UA-2's namespace rules stall the rest of UA-2 | They are a staged gap by name under 8.2, and the UA-2 rules that do not need them run |
