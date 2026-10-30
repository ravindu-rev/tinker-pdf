# Tagged PDF and accessibility

A document that carries logical structure gives it up: the engine reads the
structure tree (ISO 32000-1, 14.7), associates each structure element with the
content it tags through the marked-content machinery the interpreter runs, and
exposes a structured view — reading order, alt text, actual text, language,
artifact exclusion — built over the **same** `TextPage` the flat extraction
produces. Over the veraPDF corpus, the counts of elements found, MCIDs matched
and orphans left are ratcheted in `corpus/ratchet.json`, and the corpus's
PDF/UA (ISO 14289) fixtures are censused in `crates/tinker-pdf/tests/pdfua.rs`
through the PDF/UA validator ([pdfua.md](pdfua.md)). The writer tags its own
output through a general tagging API ("The writer" below).

## Scope

- **Structure tree reading** (14.7.2): the catalog's `/StructTreeRoot`, the
  `/K` kid recursion, `/MarkInfo` (14.7, the mark information dictionary),
  the `/RoleMap` and standard structure types (14.7.3, 14.8.4).
- **Content association** (14.7.4): integer MCID kids and `/MCR`
  marked-content references (14.7.4.2), `/OBJR` object references (14.7.4.3),
  and the `/ParentTree` number tree with per-page `/StructParents`
  (14.7.4.4) — read with the number-tree walker in
  `crates/tinker-pdf-cos/src/trees.rs` (7.9.7).
- **MCID through the interpreter**: `BDC` property lists (14.6.2) surface
  their `/MCID` — inline and `/Properties`-named forms — across the `Device`
  seam, which ruling 7 makes the only seam
  ([../rulings.md](../rulings.md)).
- **Structured extraction**: a reading-order view over `TextPage`
  (`crates/tinker-pdf-content/src/text.rs`), never a second extractor.
- **Accessibility text** (14.9): `/Alt` (14.9.3), `/ActualText` (14.9.4),
  `/E` abbreviation expansion (14.9.5), `/Lang` (14.9.2), from structure
  elements and from marked-content property lists alike.
- **Measurement**: `tpdf probe` and the corpus ratchet carry structure
  columns; the PDF/UA fixtures in the veraPDF corpus are checked against their
  pass/fail annotations for exactly the clauses implemented.
- **Writing**: `/StructParents` allocation and a `/ParentTree` on
  `DocumentBuilder`, and the tagging API built on them.

## Non-goals

- **A PDF/UA verdict engine.** That is [design/pdfua.md](pdfua.md), which
  reads the tree through this design's reader; this design reads structure.
- **Auto-tagging.** No structure is inferred for untagged documents; an
  untagged file reports "no structure tree", not a guess. Writing a new
  document's tree for an untagged one is [ROADMAP](../ROADMAP.md) row SD-07;
  reading reports what the file states.
- **The basic layout model** (14.8.3) and structure attributes (14.8.5)
  beyond the `/Table` owner's and the Layout owner's `/WritingMode`:
  `/Placement` and `/BBox` are parsed no further than storage requires, and
  Table attributes reached through `/C` and the `/ClassMap` are not read.
  [ROADMAP](../ROADMAP.md) row SD-08.
- **Assistive-technology integration.** This is bytes-to-values reading; a
  screen-reader bridge belongs to an embedder. The accessibility tree a bridge
  consumes is [ROADMAP](../ROADMAP.md) row SD-09.

## Design

### The structure tree is a facade module

`crates/tinker-pdf/src/structure.rs`, beside `optional.rs` and shaped like
it: bound once from the catalog, COS types kept inside the facade (ruling 11),
nothing mutated. `Document::structure()` returns `Option<StructureTree>` —
`None` when there is no `/StructTreeRoot`, which is most documents. The walk
over `/K` keeps the discipline `trees.rs` demonstrates: a visited set per path
for cycles, `limits::MAX_NEST_DEPTH` and an element cap, and a typed warning
with provenance on every truncation (ruling 10, `WarningKind` in
`crates/tinker-pdf-cos/src/warn.rs`).

Element types are kept twice: the raw name the file wrote, and the standard
type it resolves to after the `/RoleMap` (14.7.3) — resolution is iterative
with a cycle bound, and an unmapped custom type resolves to itself rather
than erroring (ruling 2: degrade, don't fail). Kids are an enum:
`Element`, `Content { page, mcid }` (from an integer kid on the element's
`/Pg`, or an `/MCR`), and `Object(ObjRef)` (from `/OBJR`) — three shapes the
spec gives, never collapsed, in the spirit of ruling 6.

### MCID crosses the Device seam

`Device::begin_marked_content` (`crates/tinker-pdf-content/src/device.rs`)
carries the sequence's property list, read by the interpreter
(`crates/tinker-pdf-content/src/interpret.rs`) inside the existing
marked-content path rather than beside it, in two forms:

- **Inline property lists.** `/P << /MCID 3 >> BDC` is the common form, and
  the tokenizer flattens the dictionary to `DictOpen`/`DictClose` tokens on
  the operand stack. A bounded reassembly scan reads plain values —
  `/MCID`, and the `/ActualText`, `/Alt`, `/Lang`, `/E` that 14.9 allows in
  a property list — with no COS involved, because an inline dictionary is
  only tokens. A malformed list yields no MCID and a visible scope: the
  failure direction is the one already ruled.
- **Named property lists.** `/P /MC0 BDC` resolves through `/Properties` —
  the same resource seam `FontSource::optional_content` uses.
  `FontSource::marked_content_properties(&self, name) -> Option<MarkedProps>`
  is implemented in `crates/tinker-pdf/src/resources.rs`, keeping COS reads in
  the facade exactly as the `Layer` lookup does.

The parameter is `props: Option<&MarkedProps>` — `MarkedProps` carries the
`/MCID`, `/ActualText`, `/Alt`, `/Lang` and `/E` as plain values, no COS.
The trait method's default ignores it; the interpreter's scope counting, the
over-cap ledger and the form-XObject save/restore of
`marked`/`marked_over_cap` do not depend on it.

### One TextPage, two views

`TextDevice` tracks marked-content scopes — its artifact counter implements
14.8.2.2's exclusion — and records, per `TextChar`, the innermost MCID in
scope (`Option<u32>`, `None` outside any marked sequence) and the outermost
`/ActualText` span open (`TextChar::stated`). `TextPage` is the flat
extraction's; `plain_text()` and `search()` do not read the MCID, and the
facade puts each span's text where its glyphs stand before either reads the
page, so a property list's `/ActualText` replaces what it encloses in both
views.

The structured view is a join, computed in the facade:
`Page::structured_text()` walks the page's slice of the structure tree
depth-first — which **is** reading order for tagged PDF (14.8) — and pulls,
for each `Content { mcid }` kid, the chars of the same `TextPage` carrying
that MCID. `/ActualText` replaces the enclosed content (14.9.4); `/Alt`
surfaces on the element (a `Figure`'s description); artifacts are absent
because `TextDevice` already dropped them. Chars whose MCID no structure
element claims are reported as an orphan count, not silently appended — the
parent tree (14.7.4.4) is read to cross-check that count, and a
disagreement between `/ParentTree` and the `/K` walk is a warning naming
the element (ruling 10), with the `/K` walk winning.

The flat extraction reads the tree for one thing: the direction a line is
read in, where its sequences' elements declare one (14.8.5.4.2). A char's
`(stream, MCID)` finds its element through that stream's `/StructParents`
entry of the `/ParentTree`, looked up by `/Limits`
(`tinker_pdf_cos::number_tree_lookup`) rather than walked, and the
element's `/WritingMode` is read, or inherited along `/P`
(`structure::declared_directions`).

### Measurement is the corpus harness

`tpdf probe` (`tools/tpdf/src/main.rs`) is the child `cargo xtask
corpus-run` spawns per file, and it prints a `tagged tree` line (elements,
content and object kids) and a `tagged chars` line (matched, orphans,
unmarked). `corpus/ratchet.json` carries the aggregated `tagged` bars per
corpus — files, elements, matched and orphans — so a regression in how many
elements the veraPDF corpus yields fails `corpus-run --check` the same way a
lost render does.

The veraPDF corpus encodes pass/fail per file in its paths, and the PDF/UA
census compares those annotations with the clauses the validator decides
(below). Ruling 13 leaves no fallback if those annotations prove too coarse:
they are the only outside statement about these files this design may use, so
a clause they do not cover is measured by this engine alone and the feature
doc says which.

### Writer groundwork

`DocumentBuilder` (in `tinker-pdf-cos`) allocates `/StructParents` keys and
emits a `/ParentTree` consistent with the marked content its pages carry
(14.7.4.4), so a builder-produced document round-trips through
`Document::structure()`. Fingerprinted outputs stay deterministic
([../features/determinism.md](../features/determinism.md)); extraction
semantics stay with [../features/content-and-text.md](../features/content-and-text.md).

## Milestones

1. Structure tree walk: `structure.rs`, `/StructTreeRoot`, `/K`, `/RoleMap`,
   `/MarkInfo`.
2. MCID and 14.9 properties across the `Device` seam, inline and
   `/Properties`-named.
3. `TextChar` MCID and `Page::structured_text()`: reading order, `/Alt`,
   `/ActualText`, `/E`, the orphan count.
4. Corpus graduation: `tpdf probe` structure lines and ratchet bars over the
   veraPDF corpus.
5. A PDF/UA census against the corpus annotations, scoped to the implemented
   clauses, in `crates/tinker-pdf/tests/pdfua.rs`.
6. `/StructParents` and `/ParentTree` emission on `DocumentBuilder`.

## Measuring PDF/UA

**Most of ISO 14289 is not decidable by a reader.** The clauses are about
whether tagging is *correct* — whether a `/P` is really a paragraph, whether
the reading order is the author's, whether an `/Alt` describes the picture.
Those are judgements about meaning. What a structure-tree reader can decide
is the remainder: the tree exists and walks, `/MarkInfo` says what it
should, an identifier is present, a `/Figure` has *some* alternative text, the
heading levels form an outline, a language is stated, a font is embedded.

So the census abstains on most non-conforming fixtures, and an abstention
count is not a quality that can regress. A ratchet bar over it would rise
when the engine got worse at nothing in particular, and reporting it as
"agreement" would count silences as successes, which is exactly how a
validator comes to claim a conformance level it has not earned. The census is
stricter in the direction that matters:

- **Zero false alarms is an assertion, not a bar.** A `-pass-` file that trips
  a rule fails the test outright. The cost of a false accusation is a caller
  who stops believing the true ones.
- **The caught count is a floor** recorded from a run, so a rule that quietly
  stops firing is caught.
- **Abstention is printed as abstention**, per clause, never as agreement.

**Heading levels carry across siblings and out of containers.** 7.4.2 is
about the outline a reader hears, and that outline is the headings in reading
order (14.8) regardless of what contains them.
`PDF_UA-1/7.4 Headings/7.4.2 Numbered headings/7.4.2-t01-pass-d.pdf` is a
conforming file whose H1, H2 and H3 sit in three *sibling* `Sect` elements,
and a rule that carried the deepest level only downwards would call its H2 a
skip.

## The writer

The tagging API, family by family:

- **14.9's properties and the general API.** `Tag` carries a structure type
  and, each only when stated, `/Alt`, `/ActualText`, `/E`, `/Lang` and `/T`,
  written as text strings for the declared version.
  `PageBuilder::tagged_with(&Tag, …)` is the closure form;
  `open_tag(&Tag)`/`close_tag()` the explicit one, which spans drawing calls
  and pages — an element open when its page is pushed is closed there and
  reopened on the next page begun under a key of the builder's own, and the
  halves merge at `finish` the way `tagged_keyed`'s do. An element that draws
  nothing is kept when it carries a property or `Tag::keep_empty()`, and
  dropped when it says nothing. `DocumentBuilder::set_language` writes the
  catalog's `/Lang`. The page-level nesting cap is one below the reader's,
  because `/Document` is a level of its own.
- **`/RoleMap`.** `DocumentBuilder::map_role` maps a custom type to a
  standard one, possibly through another custom type, and refuses to remap a
  standard type (ISO 14289-1 7.1), an identity, a second target, a loop, and
  an entry past the 4 096 one dictionary carries to this crate's reader
  (`limits::MAX_DICT_ENTRIES`). `STANDARD_STRUCTURE_TYPES` lives in the
  writer's crate and the PDF/A validator's level A rule reads the same list,
  so the two cannot disagree about what "standard" means.
- **`/Link` with its `/OBJR`.** A `link` call made while an element is open
  makes the annotation a content item of it; `link_for(key, …)` does so by
  `tagged_keyed`'s key, for links measured after drawing. `finish` folds the
  document tree *before* writing the pages, because an annotation in the tree
  needs its own `/StructParent` key, after every page's, whose `/ParentTree`
  value is a reference to its element rather than an array; merging allocates
  nothing, so folding early changes no object number in a document without
  such a link. `close_marked`'s take-back of an empty sequence removes
  the kid that named the id, which after a `link` call is not the element's
  last.
- **Table attributes and identifiers.** `Tag::id` writes `/ID`, claimed in
  reading order before an element's kids are written so the first element
  to carry one keeps it, and `finish` writes the `/IDTree` through the name
  tree writer the named destinations use, and the `/ParentTree` through its
  number-tree twin — one leaf while the keys fit and a balanced tree past
  sixty-four. **Limit:** past the 2^18 identifiers the tree writer keeps
  (`limits::MAX_TREE_ENTRIES`), the `/IDTree` is left out while every element
  keeps its `/ID`, which Table 322 does not allow; it takes more elements than
  the reader's own element cap reads, so the fix is a document-wide element
  bound on the writer rather than an identifier one. `Tag::table(TableAttributes)`
  writes one attribute object owned by `/Table` directly in `/A`. The reader
  reads `/A` (a dictionary, a stream, or an array with revision numbers) for
  the `/Table` owner into the same `TableAttributes` type, and `/C` classes
  are not read for it. The Layout owner's `/WritingMode` is, in `/A` and then
  in the classes `/C` names, inherited along `/P`, for the direction a line
  is read in (`structure::declared_directions`,
  [content-and-text](../features/content-and-text.md)).
- **The EPUB's pictures and languages.** The structure emission is in
  `epub/tagging.rs`. `<img alt>` is a `/Figure`'s `/Alt`, an empty `alt` an
  artifact; a picture is drawn in painting order and placed in reading order
  by a position recovered from the element tree, since the layout stamps none
  on a replaced box, with the paragraph around it split by
  `PageBuilder::continue_at` where the picture falls between two of its runs.
  `dc:language` is the catalog's `/Lang`, `xml:lang`/`lang` an element's, and
  the elements at a chapter's top carry the chapter's own language where it
  differs from the book's.
- **The EPUB's links.** Each link annotation is added with `link_for` under
  its `<a>`'s key, and an `<a>` is a `/Link` exactly when the chapter holds
  at least one annotation for it — decided over the whole chapter, so both
  halves of a link broken over a page are one type.
- **The EPUB's tables.** `summary`, `scope`, `headers`, `colspan` and
  `rowspan` become Table 349's attributes, and a cell's `id` its `/ID`,
  qualified by its content document's path so identifiers stay unique across
  chapters. A `headers` id naming no cell is dropped rather than written as a
  reference into nothing.
- **The EPUB's role map.** Every element name whose standard type is not its
  own spelling is registered with `map_role` before a chapter's pages are
  drawn and written as itself. A name `map_role` refuses because the map is
  full is written as its standard type and counted by
  `ArchiveWarning::ElementNamesUnmapped`.
- **The census over this engine's own output.** `pdfua.rs` runs the PDF/UA
  validator over an EPUB this engine converted and over a document built with
  the tagging API, and asserts exactly what fires: no `pdfuaid` claim,
  no metadata stream and no `/DisplayDocTitle` (the writer writes no PDF/UA
  packet), a link annotation with no `/Contents` and no `/Alt` on its `Link`
  element on a page with no `/Tabs /S`, and the unembedded standard 14.
- **PDF 2.0 namespaces.** `DocumentBuilder::add_namespace` (2.0 only),
  `Tag::namespace` and `map_role_in` write `/NS`, the root's `/Namespaces` and
  each namespace's `/RoleMapNS`, from what the PDF Association's approved
  errata to ISO 32000-2 quote (Tables 354 and 355, 14.8.6.1, 14.8.6.2's
  EXAMPLE 1) and veraPDF's published PDF/UA-2 rules 8.2.4-3 and 8.2.4-4 for
  what a mapping may not do. Namespace dictionaries are numbered at `finish`
  before any element, so a document with none is numbered as it would be
  without them. The reader follows `/RoleMapNS` and reports the namespace each
  type landed in, or `None` where nothing quoted says; whether the global
  `/RoleMap` is withheld from an element that names a namespace is not quoted
  either, so it is applied there, as 1.7 does. The 2.0 namespace's own type
  list is not in a source this build could read, so the writer checks a target
  only in the 1.7 namespace.
- **Associated files on elements.** `Tag::associated_file` writes the
  element's `/AF`; the file is a property the element states, so an element
  holding nothing else is kept, and two halves of one element hold the first
  half's files once. The reader's `StructElement::associated_files` shares one
  walk-wide budget with `/Headers` and `/A` (`MAX_STRUCTURE_VALUES`, 2^20
  entries, `StructureWarning::ValuesCapped`), so an array many elements share
  counts against one budget however often it is read; every string the walk
  copies — namespace URIs, `/ID`, `/Headers` ids, files' names — counts
  against `MAX_STRUCTURE_BYTES` (`StructureWarning::BytesCapped`); and a
  namespace's `/RoleMapNS` is looked up a type at a time and never copied.

Not written, each named in a refusal table rather than absent: a link wrapped
across lines is one annotation per rectangle rather than ISO 32000-2 erratum
133's one annotation with `/QuadPoints`
([content-and-text](../features/content-and-text.md)); Table attributes
reached through `/C` and the `/ClassMap` are not read (the same); an empty EPUB table
cell is not in the tree ([epub](../features/epub.md)).

## Dependencies

- `crates/tinker-pdf-cos/src/trees.rs` — `number_tree` for `/ParentTree`;
  `limits`, `WarningKind` for bounds and provenance. No new crate: structure
  is document semantics and lives in the facade (rulings 8 and 11).
- `crates/tinker-pdf-content` — `Device::begin_marked_content`,
  `FontSource`, `TextDevice`/`TextPage` (rulings 2, 7, 10).
- Corpus harness — `xtask/src/corpus.rs`, `ratchet.rs`, `tools/tpdf`
  probe records; the fetched veraPDF corpus pinned in `corpus/corpora.lock`.
- The writer — `DocumentBuilder` and the write path in `tinker-pdf-cos`; the
  strict structural validator beside it (ruling 13).

## Risks

| Risk | Mitigation |
|---|---|
| Adversarial structure trees: `/K` cycles, role-map loops, thousand-deep nesting | The `trees.rs` discipline — visited set, depth and count caps, typed truncation warnings — plus fuzz coverage; a refused subtree degrades, never panics (rulings 1, 2) |
| Inline property-list reassembly misreads flattened tokens | The scan is bounded, reads plain values only, and its failure mode is "no MCID, scope visible" — asserted by the malformed-list tests, the direction already ruled for `/OC` |
| `/ParentTree` and the `/K` walk disagree in real files | The `/K` walk is authoritative; the disagreement is a counted, provenance-carrying warning (ruling 10) and a ratcheted corpus number |
| Structured view drifts from flat extraction | One `TextDevice`, one `TextPage`; the structured view is a join over it and the flat answers do not read the MCID |
| Corpus structure counts are noisy across runs | Counts are deterministic per file (ruling 4 discipline; no wall-clock inputs) and compared as ratchet bars, not per-file assertions — the same mechanism that keeps the render bars stable |
| veraPDF corpus annotations cover clauses this design does not implement | The census scores only the clauses the PDF/UA validator decides and prints the rest as abstention by name ([design/pdfua.md](pdfua.md)) |
