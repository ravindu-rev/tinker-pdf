# PDF/UA validation

When this is done, `Document::validate_pdfua` answers "does this file conform
to ISO 14289-1 (PDF/UA-1) or ISO 14289-2 (PDF/UA-2), and which clause did it
break?" with the typed findings and the stated coverage `validate_pdfa`
already gives, for exactly the clauses a reader can decide — and it
**abstains by name** on the rest, because most of ISO 14289 is a judgement
about meaning that no reader can make. The 434 PDF/UA fixtures in the veraPDF
corpus graduate from a `#[ignore]`d census in `crates/tinker-pdf/tests/pdfua.rs`
to a ledger with the discipline `pdfa_ledger.tsv` has, and the roadmap row —
"a rule group that decides the decidable clauses and abstains by name on the
rest" — leaves.

The row is sized L. **Much of it is not new**: PDF/A level A landed in
September 2026 and validates the logical structure, `/StructTreeRoot`, the
role map against 14.8.4's standard types and every `/Lang` the object graph
carries; the PDF/A font group answers most of what 14289's font clauses ask;
and the annotation model, the XMP parser and the structure-tree reader all
exist. This document says what is shared and what is genuinely new, because
that distinction is what decides whether the roadmap schedules an L or two
halves, the first of them M.

## Scope

- **Both parts.** ISO 14289-1:2014, whose corpus clauses are 5 and 7.1 to
  7.21, and ISO 14289-2:2024, whose corpus clauses are 5 and 8.2 to 8.14 and
  which is defined on ISO 32000-2.
- **Flavour detection**: `pdfuaid:part` from the XMP packet, and for UA-2
  `pdfuaid:rev` as a four-digit year — the local reader `tests/pdfua.rs`
  already has, kept separate from `pdfa/xmp.rs`'s reader for the reason its
  doc comment gives: reading one identification schema through the other is
  what once made 434 PDF/UA files read as PDF/A claims.
- **The bar**: the veraPDF corpus's PDF/UA fixtures, whose expected verdict
  is in the filename and whose expected *message* is in the document outline.
  Data, admissible under ruling 13 ([rulings.md](../rulings.md)); the tool
  that produced them is not invoked.
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
  heading is a heading, whether colour contrast suffices. `pdfua.rs`'s own
  doc says these "are not implemented and never will be by a reader alone",
  and this design keeps that sentence. Every such clause is an `undecidable`
  entry with its reason, printed as abstention and never counted as
  agreement.
- **Auto-tagging.** No structure is inferred for an untagged file; it is
  reported as untagged. [design/reading-order.md](reading-order.md) infers an
  order and labels it inferred, and nothing it infers reaches a verdict here.
- **Fixing a file.** No repair mode; validation reports and the tagged writer
  (`PageBuilder::tagged`) conforms or refuses, as the PDF/A profile does.
- **A full PDF 2.0 namespace model.** ISO 14289-2's role maps are namespaced
  (`/RoleMapNs`, `/NS`), and [design/tagged-pdf.md](tagged-pdf.md) names PDF
  2.0 namespaces a non-goal that [pdf20-deltas.md](../pdf20-deltas.md) tracks.
  The UA-2 rules that need them are staged behind that roadmap row, not
  designed around it here.
- **Assistive-technology output.** Bytes to verdicts; a screen-reader bridge
  is an embedder's.

## What the corpus carries, measured

Counted with a directory listing on 16 September 2026 over
`corpus/files/verapdf`, which holds 2 907 files.

| Part | Fixtures | `-pass-` | `-fail-` |
| --- | ---: | ---: | ---: |
| `PDF_UA-1` | 296 | 141 | 155 |
| `PDF_UA-2` | 138 | 54 | 84 |
| **both** | **434** | **195** | **239** |

By clause directory:

| UA-1 clause | files | pass | fail | | UA-2 clause | files | pass | fail |
| --- | ---: | ---: | ---: | --- | --- | ---: | ---: | ---: |
| 5 Version identification | 10 | 5 | 5 | | 5 Version identification | 7 | 1 | 6 |
| 7.1 General | 30 | 14 | 16 | | 8.2 Logical structure | 49 | 21 | 28 |
| 7.2 Text | 110 | 50 | 60 | | 8.4 Text representation for content | 67 | 29 | 38 |
| 7.3 Graphics | 5 | 3 | 2 | | 8.6 Text string objects | 1 | 0 | 1 |
| 7.4 Headings | 14 | 7 | 7 | | 8.7 Optional content | 2 | 1 | 1 |
| 7.5 Tables | 8 | 5 | 3 | | 8.8 Intra-document destinations | 2 | 0 | 2 |
| 7.7 Mathematical expressions | 5 | 3 | 2 | | 8.9 Annotations | 2 | 0 | 2 |
| 7.9 Notes and references | 5 | 2 | 3 | | 8.10 Forms | 2 | 0 | 2 |
| 7.10 Optional content | 5 | 2 | 3 | | 8.11 Metadata | 5 | 2 | 3 |
| 7.11 Embedded files | 3 | 1 | 2 | | 8.14 Use of embedded files | 1 | 0 | 1 |
| 7.15 XFA | 1 | 0 | 1 | | | | | |
| 7.16 Security | 2 | 1 | 1 | | | | | |
| 7.18 Annotations | 48 | 25 | 23 | | | | | |
| 7.20 XObjects | 4 | 2 | 2 | | | | | |
| 7.21 Fonts | 46 | 21 | 25 | | | | | |

**Every fixture but eighteen states its own expectation in its outline**, and
the eighteen that do not are all `-pass-` twins. The outlines of all 434 were
read with this repository's own `tpdf outline` on the same day; they are the
requirement source below wherever the clause text alone would not have given
the rule, and several of them overturn a reading the clause text invites.

**Where the census stands today**, from `cargo test -p tinker-pdf --test
pdfua -- --ignored` on 16 September 2026 with `TINKER_CORPUS` set:

| | |
| --- | ---: |
| annotated files | 434 |
| conforming | 195 |
| non-conforming, caught | 29 |
| non-conforming, abstained | 210 |
| could not be opened | 0 |
| **false alarms** | **0** |

Nine rules fire: no structure tree (3), not marked (1), suspects (1), no
`pdfuaid:part` (4), figure without alt (2), heading level skipped (2), no
natural language (12), font not embedded (3), tree not walkable (2). The
per-clause abstentions are where the work is: 7.2 Text abstains on 52 of 60,
7.18 Annotations on 23 of 23, 7.21 Fonts on 24 of 25, 8.2 on 24 of 28, 8.4 on
33 of 38.

**The 7.21 number is the one to read first.** Twenty-four of twenty-five font
fixtures abstain, and the PDF/A font group in `crates/tinker-pdf/src/pdfa/fonts.rs`
already answers most of what they ask — embedding, `CIDSystemInfo` agreement
between a Type 0 font and its descendant, `CIDToGIDMap`, a symbolic TrueType
font carrying an `/Encoding`, a non-standard base encoding, a missing
`ToUnicode` with the four exemptions — under ISO 19005's clause numbers. It
does not run on these files because none of them claims PDF/A. That is a
flavour key, not a rule.

## What is shared, and what is new

Read across the 416 outlines, every fixture falls into one of four columns.
The first two are what makes half of this row an M; the last two are the L.

### Shared: rules that exist and want a second clause table

| Requirement, as the fixtures state it | Where it exists | Fixtures |
| --- | --- | --- |
| `/StructTreeRoot` present; `/MarkInfo /Marked true`; `/Suspects` not true | `pdfa/logical.rs`, `pdfua.rs` | 7.1-t04, 7.1-t11, 8.2.1 |
| Every structure type resolves through the `/RoleMap` to a standard type, case-sensitively; no remap to the empty string; two non-standard types mapped to each other; `Document` remapped to `Book` | `pdfa/logical.rs` structure-type rule over `crate::structure::bind` | 7.1-t05 to t07, 8.2.4-t01, t02, t04 |
| `/Lang` well formed: primary subtag 1 to 8 letters (`portugue` passes, `portugues` fails), a digit primary fails, a Cyrillic subtag fails, `1234abcd` passes and `1234abcde` fails | `pdfa/logical.rs::language_is_well_formed`, the RFC 3066 form | 7.2-t29 (26 files), 8.4.4-t02 (26 files) |
| Fonts: embedding (7.21.4.1), `CIDSystemInfo` `Registry`/`Ordering` equal and `Supplement` not exceeding (7.21.3.1), `CIDToGIDMap` present and not a non-Identity name (7.21.3.2), a predefined CMap in ISO 32000-1 Table 118 and a `UseCMap` likewise (7.21.3.3), a non-symbolic TrueType font's `/Encoding` with a `/BaseEncoding` of `WinAnsiEncoding` or `MacRomanEncoding`, a symbolic one with no `/Encoding` (7.21.6), `ToUnicode` with its exemptions (7.21.7-t01) | `pdfa/fonts.rs`: `FontNotEmbedded`, `CidSystemInfoIncomplete`, `CidToGidMapMalformed`, `EncodingNotStandard`, `SymbolicFontHasEncoding`, `ToUnicodeMissing` | 7.21 and 8.4.5, most of 46 + 67 |
| Optional content configuration: `/D` carries a non-empty `/Name`; no `/AS` | ISO 19005-2 6.9 asks the same; staged in `PDFA_STAGED` | 7.10, 8.7 |
| Embedded file specification carries `/F` and `/UF` | `pdfa/syntax.rs::EmbeddedFileKeyMissing` | 7.11, 7.18.7 |
| Dynamic XFA forbidden (`dynamicRender` = `required`) | `XfaForbidden` forbids all XFA; the UA rule is narrower and reads the XFA packet | 7.15 |
| A metadata stream exists; `dc:title` present; `/ViewerPreferences /DisplayDocTitle true` | the XMP pull parser in `pdfa/xmp.rs`; the rule is new and small | 7.1-t08 to t10, 8.11 |

**One shared grammar, split by one flag.** The level A group admits
`/Lang ()` because `6-8-4-t01-pass-d` says the empty string "is permitted";
`7.2-t29-fail-n`, `-fail-o` and `-fail-p` say the opposite for PDF/UA —
"Value of Lang is empty" is annotated `fail` in the catalog, on a `/P` and in
a marked-content sequence. Same function, one parameter, two standards; and
the corpus is what says so, not either clause.

### Shared: a fixture-decided detail the clause text would not give

- **An empty `/ActualText` is a pass and an empty `/Alt` is a fail** on a
  `Figure` (7.3-t01-pass-c against -fail-b) and on a `Formula`
  (7.7-t01-pass-c against -fail-b). `figure_without_alt` today treats the two
  keys alike and must not.
- **An annotation with the hidden flag, or whose `/Rect` lies outside the
  page, needs no `/Contents` and no `/Alt`** (7.18.1-t02-pass-c, -pass-d); a
  widget's `/TU` is read from the **field**: a field carrying one passes
  (7.18.1-t03-pass-e) and a field with none whose two widget kids each carry
  one fails (-fail-d).
- **A media clip's `/CT` may be empty and still pass**; its `/Alt` may not
  (7.18.6.2-t01-pass-a, -t02-fail-b).
- **`FREETEXT` in upper case is "not standard"** and the fixtures that carry
  it are annotated `pass` — the annotation census in
  `crates/tinker-pdf/tests/annotation_census.rs` already names five such
  subtypes as `AnnotationKind::Other`, and this group must not report them.
- **A `List` may contain a `List`** (7.2-t17-pass-g) while "L is present in
  List item" fails (7.2-t20-fail-b): the two fixtures draw the line, and the
  rule follows them rather than the clause's wording.

### New: decidable from the tree, with machinery `structure.rs` does not have

| Requirement | What it needs |
| --- | --- |
| Table content model: only `THead`, `TBody`, `TFoot`, `TR`, `TH`, `TD` and `Caption` directly in `Table`; one `Caption`, first or last; at most one `THead` and one `TFoot`, neither without a `TBody`; `TH`/`TD` inside `TR`; `TR` inside `Table` or a row group; no `Span` directly in a row group (7.2-t03 to t14, t36 to t38) | `standard_type` and `kids`, which `StructElement` has |
| List and TOC models (7.2-t17 to t20, t26, t27); headings numbered without skips and starting at H1, unnumbered `H` one per `Sect` and never mixed with `Hn` (7.4); `H` forbidden under UA-2 (8.2.5.12); `Math` inside `Formula` (8.2.5.29); a `Document` element at the root (8.2.5.2) | the same |
| Table regularity — `RowSpan`/`ColSpan` consistent across the grid (7.2-t41 to t43, 8.2.5.26-t03, t04); header association — every `TD` reachable through a `Scope` on a `TH` or through `/Headers` naming an `/ID` (7.5, 8.2.5.26-t05, t06); `ListNumbering` not `None` (8.2.5.25); `Note` elements carry unique `/ID`s (7.9) | **an attribute reader.** `structure.rs` reads no `/A` dictionary and no `/ID` — a search for either finds nothing — so `/RowSpan`, `/ColSpan`, `/Scope`, `/Headers`, `/ListNumbering` and `/ID` are a new bounded walk. The grid model this needs is the same one [design/table-reconstruction.md](table-reconstruction.md) scores against, and is built once |
| Annotations inside the structure tree: every annotation an `/OBJR` kid of an element; `Link` annotations inside a `Link` element, role-mapped or not; widgets inside `Form`, at most one per `Form` under UA-2; `/Contents` or `/Alt` with the exemptions above; `Tabs /S` on every page with annotations (`/C` and `/R` fail); `TrapNet` forbidden; `PrinterMark` an artifact and not in the tree; invisible annotations artifacts (8.9.2.2); `AFRelationship` on attachments (8.9.2.4.10) | `StructKid::Object(ObjRef)` exists; the join to `Page::annotations()` (28 typed subtypes, `/F`, `/Contents`, `/Popup`) is new |
| Form XObject `/Ref` forbidden (7.20-t01); encryption `/P` bit 10 set (7.16); PUA code points in `/Lang` (8.6; the published UA-2 profile lists 8.4.3 for text strings generally, and the corpus has no fixture for it); structure destinations in outlines and `GoTo` actions (8.8); `Link` elements whose annotations target different destinations (8.2.5.20) | COS reads; destinations already an enum (ruling 6) |
| Namespaced role maps and the PDF 2.0 namespace on `Document` (8.2.4-t02-c, -t03, 8.2.5.2-t02) | PDF 2.0 namespaces — a tier 3 roadmap row, not this one |

### New: the real-content group, which needs the interpreter

Eight `-fail-` fixtures ask a question no walk over dictionaries answers.
7.1-t01 to t03 and 8.2.2: *every* piece of content a page draws is either inside an
`/Artifact` marked-content scope or inside a scope carrying an `/MCID` that
some structure element claims; an `/Artifact` inside real content fails; a
page whose content stream never distinguishes an artifact fails; an image
"not marked as Artifact or real content" fails. 7.20-t02: a form XObject that
carries an `/MCID` and is invoked three times fails, invoked once passes.

`pdfa/content.rs` is a tokenizer by design — it tracks the text rendering
mode and the selected font and nothing else, and its doc says a rule that
needs more "would need `tinker_pdf_content::interpret` and a `Device`". This
is that rule. The seam is the one ruling 7 makes the only seam, and the
device is the one promoted this week: `RecordingDevice`
(`crates/tinker-pdf-content/src/record.rs`) under `Capture { text: true,
paths: true, images: true, structure: true, state: false }`. For every
`ShowGlyph`, `FillPath`, `StrokePath`, `DrawImage` and `DrawShading` event,
`scopes_at(index)` gives the scopes open at the call, outermost first; a scope
tagged `Artifact` settles the first question, and a scope whose
`MarkedScope::props` carries an `/MCID` is looked up in the set of
`(stream, mcid)` pairs the tree claims — the set `StructureTree::text_for_page`
already computes for characters, generalised to every event. `BeginForm { id }`
events counted per `id` settle 7.20-t02.

What the recorder does not give this group, stated so the milestone is priced
right: annotation appearance streams are drawn by the reader (12.5.5) and
`Page::text()` interprets the page's stream and the forms it invokes, never an
`/AP` — the content walk in `pdfa/content.rs` visits appearances and the
interpreter does not, so an appearance stream's real-content question is
answered by a second interpretation per appearance or not at all, and the
first delivery says which. And the recorder records; it does not decide. The
join, the claimed set and the two exemptions the fixtures state (7.1-t01-pass-b:
an artifact inside a `/P` that has no `/MCID` passes; 7.1-t02-pass-b: "junk"
inside an artifact with no `/MCID` passes) are this group's to write.

### Undecidable, and named as such

Whether tagging is *correct*. The 195 conforming fixtures and the 239
non-conforming ones both contain files whose difference is only visible to a
person — a `/P` around a heading, an `/Alt` that lies. `PDFUA_UNDECIDABLE` is
a static list keyed by clause with the reason, mirrored into the verdict, and
the census prints its members as abstention with the word *undecidable* beside
them rather than the word *staged*. The distinction matters for scheduling:
a staged clause is a milestone; an undecidable one is a limit and belongs in
the roadmap's named non-goals when the row leaves.

## Design

**Where it lives.** `crates/tinker-pdf/src/pdfua/`, beside `pdfa/`, and the
kernel `pdfa.rs` carries — `Raw`, `Machinery` with its counted reaches,
`RuleGroup`, `Coverage`, `Verdict`, the clause-table pattern, the content
walk and the font, colour and annotation visitors — moves to a module both
standards use. [design/pdfx.md](pdfx.md) asks for the same move; whichever
lands first makes it, and the other reuses it.

**One rule, two standards, two clause tables.** `ClauseTable` in `pdfa.rs`
keys a rule to a number per ISO 19005 part. A shared rule gains a second table
keyed by ISO 14289 part — 7.21.4.1 under UA-1, 8.4.5.5.1 under UA-2, for the
embedding rule that is 6.3.4 under 19005-1 — and runs whenever *either* claim
is present, reporting under the numbering of the standard whose claim it ran
for. A file claiming both is validated by both, and the two verdicts share the
finding kinds and nothing else. The flavour gate is per rule and per standard:
the level A group today refuses to run below level A because "a level B file
is not required to be tagged", and the same gate reads "a file that claims
neither PDF/A level A nor PDF/UA is not required to be tagged".

**`FindingKind` stays one closed enum.** A missing `Tabs /S` is a new
variant; a `Figure` without `/Alt` is the one the level A group would use if
19005 asked (it does not — 14.9.3 is a PDF/UA requirement). Adding a variant
is a reviewable change to what "conforms" means, as it is for PDF/A.

**Coverage grows a second axis.** `Coverage` says which groups ran.
`PdfUaVerdict` adds `abstained: Vec<Abstention { clause, class, reason }>`
built from `PDFUA_STAGED` and `PDFUA_UNDECIDABLE`, filtered to the part
claimed, so a caller reading an empty finding list sees, in the same struct,
the 7.2 clauses nobody decided. That is the roadmap row's "abstains by name"
made a value rather than a sentence in a doc.

**The acceptance criterion governs every milestone.** Over the 195 `-pass-`
files, **the false-alarm count is zero and stays zero, as a hard assertion**,
the one `pdfua.rs` makes today. The caught count is a floor recorded from a
run, never aimed at — the first floor written into that test was 40, guessed,
and wrong in the flattering direction. A rule that raises the floor by
reporting a conforming file has raised it for nothing; the level A group's
own injection table shows the shape (a rule made mandatory that the fixtures
make optional: twelve false positives at once).

**The ledger.** `crates/tinker-pdf/tests/pdfua_ledger.tsv`, one row per
abstained `-fail-` file, each with a class — `staged`, `undecidable`,
`reading` — and a mandatory reason; a row without a reason is refused by the
reader; a row whose subject no longer abstains fails as stale; an abstention
with no row fails as uncovered. The stale half of that assertion is what found
three engine defects in the PDF/A ledger's first week, and it is the reason
the census graduates from a printout to a ledger.

**The independent check outside the suite.** The fetched corpora carry
**1 078 tagged files** (`corpus/ratchet.json`, `tagged.files`, summed over
five corpora), of which 361 are SafeDocs documents real producers wrote. None
claims PDF/UA, so none is scored; every one is run through the structure and
real-content groups with the flavour gate lifted, and a finding on a
real-world tagged file is printed with its kind — the check the fifteen real
PDF/A-1a files give the level A group, at a scale that says whether a rule
survives contact with what producers actually write into a role map.

**The writer.** `PageBuilder::tagged` documents claim nothing about PDF/UA
today; a `pdfuaid` packet on the archival profile is one field, and every
tagged writer fixture is then judged by this validator with complete coverage
and zero findings — the same asymmetry pdfa.md records (the writer is checked
by the table that would accept its mistakes), narrowed the same way (twins),
and not closed.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | The kernel shared; `pdfua/` with the nine rules of `tests/pdfua.rs` moved in unchanged; `PdfUaVerdict` with abstentions; `Document::validate_pdfua`; `tpdf check --pdfua`; `PDFUA_STAGED` and `PDFUA_UNDECIDABLE` | The census through the facade still reads 29 caught, 0 false alarms; every clause directory that abstains has at least one entry in one of the two lists; `hostile_input.rs` calls the validator with zero panics; `Machinery` shows no font program parsed by a syntax-only PDF/UA sweep | M |
| 2 | Second clause tables on the shared rules: fonts, `/Lang` with the empty-string flag, role map and structure types, optional content, embedded files, the three metadata rules; and three small font rules with no PDF/A analogue — `WMode` agreement between an `/Encoding` CMap stream and its dictionary (7.21.3.3-t02), the forbidden `ToUnicode` values `<0000>`, `<FFFE>` and `<FEFF>` (7.21.7-t02), a `/Differences` name absent from the Adobe Glyph List (7.21.6-t02-d) | 7.21 abstentions fall from 24 to the staged residue — `CharSet` and `CIDSet` completeness, `/Widths` against the program, `.notdef` drawn, the four the PDF/A group also stages — and 8.4.5's likewise, each residue file a ledger row; `/Lang ()` reported under PDF/UA and admitted under PDF/A in one test; false alarms still zero; the caught floor re-recorded from the run | M |
| 3 | The attribute reader in `structure.rs` (`/A`, `/ID`, `/Headers`, `/Scope`, `/RowSpan`, `/ColSpan`, `/ListNumbering`) and the structure-grammar group: tables, lists, TOC, headings both kinds, `Math`, `Document`, `Note` IDs, table regularity and header association | 7.2's 60 `-fail-` files: every grammar fixture caught, the residue named per file in the ledger; 7.4's 7, 7.5's 3, 7.9's 3 and 8.2.5.26's fail files caught; `tagged.elements` in `corpus/ratchet.json` unchanged, so the reader's walk did not change what it counts; the level A bar still 830 of 831 | L |
| 4 | The annotations-in-structure group (7.18, 8.9, 8.10, 8.14) over `Page::annotations()` and `StructKid::Object` | 7.18's 23 `-fail-` files caught with every pass-twin exemption asserted by name (hidden, off-page, `/TU` on the field, empty `/CT`); the `FREETEXT` fixtures silent; false alarms zero | M |
| 5 | The real-content group over `RecordingDevice` | 7.1-t01 to t03, 8.2.2 and 7.20-t02 caught; the two `-pass-` exemptions asserted; a counted injection that records under `Capture::GLYPHS` instead — which drops every scope — fails the group's tests; the 361 SafeDocs tagged files run with the gate lifted and every finding printed with its kind | M |
| 6 | UA-2 specifics: `pdfuaid:rev`, PUA, structure destinations, `Link` targets, `AFRelationship`, and the namespaced role-map rules **behind the PDF 2.0 namespaces row** | 8.2's and 8.4's abstentions reduced to the namespace fixtures and the undecidable residue, each a ledger row | M, part of it blocked |
| 7 | The ledger, and the row's exit: `pdfua_ledger.tsv` with uncovered and stale assertions; `pdfuaid` on the archival profile; the tagged writer's fixtures judged | Zero uncovered, zero stale; every `PageBuilder::tagged` fixture in `pdfa_writer.rs` validates under PDF/UA-1 with zero findings and its untagged twin does not; the roadmap row deleted in the same commit | S |

Milestones 1, 2 and 7 are the M the row contains and can land alone; they
move the caught count by every fixture whose rule already exists. Milestones
3 to 6 are the L, and 3 is the largest because the attribute reader and the
grid model serve two designs.

## Dependencies

- **`crates/tinker-pdf/src/pdfa/`** — the kernel and the font, annotation
  and XMP machinery. Exists; must be shared rather than copied.
- **`crates/tinker-pdf/src/structure.rs`** — `bind`, the role-map
  resolution, `text_for_page`'s claimed set. Exists. **Reads no `/A`
  attributes and no `/ID`**; milestone 3 adds that walk under the same
  bounds (`MAX_STRUCTURE_DEPTH`, the element cap).
- **`crates/tinker-pdf-content/src/record.rs`** — the recording device.
  Exists. Its module doc names inferred reading order as a consumer that
  "wants glyphs and the marked-content nesting" and then lists it among the
  consumers that run under `Capture::GLYPHS`, whose `structure: false`
  records no scope at all; the real-content group here needs `structure:
  true`, and that sentence should be corrected when milestone 5 lands.
- **`Page::annotations()`** — the 28-subtype model with `/F`, `/Contents`
  and `/Popup`. Exists; the per-family payloads the roadmap lists as unread
  are not needed here.
- **PDF 2.0 namespaced structure types** — a tier 3 roadmap row; milestone 6
  waits on it for the `/RoleMapNs` fixtures.
- **The published PDF/UA profiles**, read as data: veraPDF's `PDFUA-1.xml`
  (151 rules, "Validation rules against ISO 14289-1:2014", dated 2020-04-07)
  and `PDFUA-2.xml` (127 rules, against ISO 14289-2:2024, dated 2023-11-08)
  enumerate, per clause, what their authors found decidable. They are the
  same standing the machine-readable PDF/A profiles have in
  [design/pdfa.md](pdfa.md) — a published statement about which clauses are
  checkable, admissible to read and never run.
- **The standards' text.** ISO 14289-1 and -2 are sold; the corpus's
  directory names, outlines and the profiles above are the clause map this
  design was written from, and a rule is staged rather than written where
  those three disagree.

## Risks

| Risk | Mitigation |
| --- | --- |
| **A rule that is right by the clause and wrong by the corpus.** The level A group met four of these; PDF/UA's fixtures already state five more (an empty `/ActualText`, an off-page annotation, an inherited `/TU`, an empty `/CT`, upper-case `FREETEXT`) | The pass twins are the requirement. Every rule lands with the pass fixture that would report it if it were too strict, and the false-alarm assertion is the gate — a rule that catches ten and alarms once does not land |
| **Abstention read as a pass.** A verdict with no findings from a validator that decided four clauses of forty | `PdfUaVerdict::abstained` is in the same struct as `findings`; `tpdf check --pdfua` prints both; the census prints *undecidable* and *staged* as words, never as a rate |
| The attribute walk over `/A` and `/ID` is a new bounded recursion on untrusted input | The `trees.rs` discipline the structure reader already uses — depth cap, element cap, typed truncation warning — and the `hostile_input.rs` sweep grows a `validate_pdfua` call in milestone 1, before the walk exists |
| The corpus is atomic and one-clause; a rule tuned to it reports real documents | The 361 SafeDocs tagged files and the 90 pdfjs ones, run with the gate lifted, every finding printed with its kind; a kind that fires on dozens of real files is a rule to re-read, whatever the fixtures say |
| The 18 pass fixtures with no outline have no stated reason to pass | They are annotated by name and their reason is in their `-fail-` twin's outline; a rule that reports one has no message to argue with and is wrong on the annotation alone |
| Two standards sharing one rule drift when one is edited for a fixture the other lacks | The empty-`/Lang` flag is the pattern: the split is a parameter with a test on each side, never a second function |
| UA-2's namespace rules stall behind another row and the census stalls with them | Milestone 6 is split so the non-namespace UA-2 rules land first; the namespace fixtures are `staged` ledger rows pointing at the roadmap row by name |

## As built

*Filled in as milestones land.* Nothing has landed beyond what this document
measures: the nine-rule census in `tests/pdfua.rs`, at 29 of 239 with zero
false alarms, which predates this design and is its starting point.
