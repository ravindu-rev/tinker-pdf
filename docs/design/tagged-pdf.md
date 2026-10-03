# Tagged PDF and accessibility

When this is done, a document that carries logical structure gives it up: the
engine reads the structure tree (ISO 32000-1, 14.7), associates each structure
element with the content it tags through the marked-content machinery the
interpreter already runs, and exposes a structured view — reading order, alt
text, actual text, language, artifact exclusion — built over the **same**
`TextPage` the flat extraction already produces. The 2 907-file veraPDF corpus,
today a never-crash bar ([../verification.md](../verification.md)), graduates
to a structure bar: counts of elements found, MCIDs matched and orphans left
are ratcheted in `corpus/ratchet.json`, and the PDF/UA (ISO 14289-1) cases in
that corpus are measured against the checks this design implements. This was the
roadmap's "Tagged PDF and accessibility" row; it has left
[../ROADMAP.md](../ROADMAP.md), and so, in October 2026, has the
tagged-writing row that carried the writing gaps this design named as future
work — see "The writer, as built".

## Scope

- **Structure tree reading** (14.7.2): the catalog's `/StructTreeRoot`, the
  `/K` kid recursion, `/MarkInfo` (14.7, the mark information dictionary),
  the `/RoleMap` and standard structure types (14.7.3, 14.8.4).
- **Content association** (14.7.4): integer MCID kids and `/MCR`
  marked-content references (14.7.4.2), `/OBJR` object references (14.7.4.3),
  and the `/ParentTree` number tree with per-page `/StructParents`
  (14.7.4.4) — read with the existing number-tree walker in
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
- **Measurement**: `tpdf probe` and the corpus ratchet grow structure
  columns; PDF/UA cases in the veraPDF corpus are checked against their
  pass/fail annotations for exactly the clauses implemented.
- **Writer groundwork**: `/StructParents` allocation on `DocumentBuilder` —
  a milestone, deliberately not the core of this design.

## Non-goals

- **A PDF/UA validation verdict engine.** Full conformance checking is the
  PDF/A design's territory (`design/pdfa.md` shares the machinery); this
  design measures the structure clauses it implements, nothing more.
- **Auto-tagging.** No structure is inferred for untagged documents; an
  untagged file reports "no structure tree", not a guess.
- **The basic layout model** (14.8.3) and structure attributes (14.8.5):
  `/Placement`, `/BBox`, table row/column spans are parsed no further than
  storage requires.
- **Writing complete tagged structure.** `DocumentBuilder` gains
  `/StructParents` allocation and a minimal element tree for its own output;
  a general tagging API is future work. *Amended October 2026: the general
  API landed after this design closed, as the tagged-writing row — see
  "The writer, as built" below.*
- **PDF 2.0 namespaces** (ISO 32000-2's namespaced structure types); noted
  in `docs/pdf20-deltas.md` when read support lands.
- **Assistive-technology integration.** This is bytes-to-values reading; a
  screen-reader bridge belongs to an embedder.

## Design

### The structure tree is a facade module

A new `crates/tinker-pdf/src/structure.rs`, beside `optional.rs` and shaped
like it: bound once from the catalog, COS types kept inside the facade
(ruling 11), nothing mutated. `Document::structure()` returns
`Option<StructureTree>` — `None` when there is no `/StructTreeRoot`, which is
most documents. The walk over `/K` reuses the discipline `trees.rs` already
demonstrates: a visited set per path for cycles, `limits::MAX_NEST_DEPTH` and
an element cap, and a typed warning with provenance on every truncation
(ruling 10, `WarningKind` in `crates/tinker-pdf-cos/src/warn.rs`).

Element types are kept twice: the raw name the file wrote, and the standard
type it resolves to after the `/RoleMap` (14.7.3) — resolution is iterative
with a cycle bound, and an unmapped custom type resolves to itself rather
than erroring (ruling 2: degrade, don't fail). Kids become an enum:
`Element`, `Content { page, mcid }` (from an integer kid on the element's
`/Pg`, or an `/MCR`), and `Object(ObjRef)` (from `/OBJR`) — three shapes the
spec gives, never collapsed, in the spirit of ruling 6.

### MCID crosses the Device seam

Today `Device::begin_marked_content(tag, visible, hidden_layer)`
(`crates/tinker-pdf-content/src/device.rs`) carries no property list; the
interpreter (`crates/tinker-pdf-content/src/interpret.rs`) reads `BDC`
properties only for the `/OC` name form. Two additions, both inside the
existing marked-content path rather than beside it:

- **Inline property lists.** `/P << /MCID 3 >> BDC` is the common form, and
  the tokenizer flattens the dictionary to `DictOpen`/`DictClose` tokens on
  the operand stack. A bounded reassembly scan reads plain values —
  `/MCID`, and the `/ActualText`, `/Alt`, `/Lang`, `/E` that 14.9 allows in
  a property list — with no COS involved, because an inline dictionary is
  only tokens. The scan that was written and deleted for optional content
  (documented at `interpret.rs`'s `optional_content_name`) returns with a
  purpose that changes answers. A malformed list yields no MCID and a
  visible scope: the failure direction is the one already ruled.
- **Named property lists.** `/P /MC0 BDC` resolves through `/Properties` —
  the same resource seam `FontSource::optional_content` uses today. The
  trait gains `marked_content_properties(&self, name) -> Option<MarkedProps>`,
  implemented in `crates/tinker-pdf/src/resources.rs`, keeping COS reads in
  the facade exactly as the `Layer` lookup does.

`begin_marked_content` grows a `props: Option<&MarkedProps>` parameter
(`MarkedProps { mcid, actual_text, alt, lang, expansion }` — plain values,
no COS). Every existing device ignores it by default; the interpreter's
scope counting, the over-cap ledger and the form-XObject save/restore of
`marked`/`marked_over_cap` are untouched.

### One TextPage, two views

`TextDevice` already tracks marked-content scopes — its artifact counter
implements 14.8.2.2's exclusion and is tested in `text.rs`. It additionally
records, per `TextChar`, the innermost MCID in scope (`Option<u32>`, `None`
outside any marked sequence). `TextPage` stays exactly what it is; the flat
`plain_text()` and `search()` answers do not change by a byte.

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

### Measurement is the corpus harness, extended

`tpdf probe` (`tools/tpdf/src/main.rs`) is the child `cargo xtask
corpus-run` spawns per file. It grows lines the runner already knows how to
count:
`struct elements N`, `struct mcids N`, `struct orphans N`, plus `warn`
kinds for truncation and parent-tree disagreement. `corpus/ratchet.json`
gains the aggregated bars, so a regression in how many elements the
veraPDF corpus yields fails `corpus-run --check` the same way a lost
render does today. The veraPDF corpus encodes pass/fail per file in its
paths; the PDF/UA milestone diffs those annotations against this design's
checks — tagged (`/MarkInfo /Marked true`), structure tree present, MCIDs
resolvable, `/Alt` present on `Figure` — **scoped to those clauses only**,
with every disagreement enumerated in `corpus/report.json`. Ruling 13
leaves no fallback if those annotations prove too coarse: they are the only
outside statement about these files this design may use, so a clause they do
not cover is measured by this engine alone and the feature doc says which.

### Writer groundwork

`DocumentBuilder` (in `tinker-pdf-cos`) learns to allocate `/StructParents`
keys and emit a `/ParentTree` consistent with the marked content its pages
carry (14.7.4.4). That is the whole of the writer story here: enough that a
builder-produced document round-trips through `Document::structure()`, and
the foundation the EPUB and creation paths need before a tagging API is
worth designing. Fingerprinted outputs stay deterministic
([../features/determinism.md](../features/determinism.md)); extraction
semantics stay with [../features/content-and-text.md](../features/content-and-text.md).

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size (S/M/L/XL) |
|---|---|---|---|
| 1 | Structure tree walk: `structure.rs`, `/StructTreeRoot`, `/K`, `/RoleMap`, `/MarkInfo` | Unit fixtures for cycle, depth cap, role-map loop and odd kids pass; `Document::structure()` yields a tree on a tagged veraPDF file in a named integration test; the `hostile_input.rs` suite and the document fuzz target cover the walk with no crash | M |
| 2 | MCID and 14.9 properties across the `Device` seam (inline and `/Properties`-named) | Interpreter unit tests: inline `/MCID`, named list, malformed list (no MCID, still visible), nesting under a hidden `/OC` scope; all existing `interpret.rs` and `optional_content.rs` tests unchanged | M |
| 3 | `TextChar` MCID + `Page::structured_text()` reading order, `/Alt`, `/ActualText`, `/E`, orphan count | A fixture whose content-stream order differs from structure order extracts in structure order; `/ActualText` replaces enclosed chars; `Figure` alt text surfaces; `plain_text()` output byte-identical to before on the whole fingerprint suite | M |
| 4 | Corpus graduation: `tpdf probe` structure lines, ratchet bars over the veraPDF corpus | `cargo xtask corpus-run --check` compares `struct elements` / `mcids` / `orphans` bars; `corpus/ratchet.json` committed with non-zero element counts for `verapdf`; a seeded regression (element cap set to 0) fails the check | S |
| 5 | PDF/UA measurement against corpus annotations, scoped to implemented clauses | **Amended, see below.** A `#[ignore]`d census in `crates/tinker-pdf/tests/pdfua.rs` states caught/abstained per clause over all 434 annotated fixtures; **zero false alarms is a hard assertion**; the caught count is a recorded floor; every false alarm is a named file | M |
| 6 | `/StructParents` + `/ParentTree` emission on `DocumentBuilder` | A builder-produced two-paragraph document round-trips through `Document::structure()` with both MCIDs matched and zero orphans; the strict structural validator reports the file clean | S |

## Milestone 5's amendment, and why

The row above originally asked for agreement bars in `corpus/ratchet.json`
and disagreements in `corpus/report.json`. It was written before the
measurement existed, and the measurement is not shaped the way it assumed.

**Most of ISO 14289 is not decidable by a reader.** The clauses are about
whether tagging is *correct* — whether a `/P` is really a paragraph, whether
the reading order is the author's, whether an `/Alt` describes the picture.
Those are judgements about meaning. What a structure-tree reader can decide
is the small remainder: the tree exists and walks, `/MarkInfo` says what it
should, an identifier is present, a `/Figure` has *some* alternative text, the
heading levels form an outline, a language is stated, a font is embedded.

Measured over all 434 annotated fixtures, that catches **29 of 239
non-conforming files with zero false alarms**, and abstains on 210. So the
number a ratchet bar would carry is dominated by abstention — and an
abstention count is not a quality that can regress. A bar over it would rise
when the engine got worse at nothing in particular, and reporting it as
"agreement" would count 210 silences as successes, which is exactly how a
validator comes to claim a conformance level it has not earned.

What replaces it is stricter in the direction that matters:

- **Zero false alarms is an assertion, not a bar.** A `-pass-` file that trips
  a rule fails the test outright. The cost of a false accusation is a caller
  who stops believing the true ones.
- **The caught count is a floor**, so a rule that quietly stops firing is
  caught. It was recorded from a run: the first floor written here was 40,
  guessed before the census ran, and it was wrong in the flattering direction.
- **Abstention is printed as abstention**, per clause, never as agreement.

One reading was overturned by a fixture on the way.
`PDF_UA-1/7.4 Headings/7.4.2 Numbered headings/7.4.2-t01-pass-d.pdf` is a
conforming file whose H1, H2 and H3 sit in three *sibling* `Sect` elements.
A heading rule that carried the deepest level only downwards saw each `Sect`
start from nothing and called the H2 a skip. 7.4.2 is about the outline a
reader hears, and that outline is the headings in reading order (14.8)
regardless of what contains them — so the level carries across siblings and
out of containers. The corpus found that; the unit tests, all of which used
flat siblings, could not have.

## The writer, as built

Milestone 6's builder wrote a type and the content it claimed, and nothing
else. The roadmap's tagged-writing row grew it into a tagging API; what it
writes, family by family:

- **14.9's properties and the general API.** `Tag` carries a structure type
  and, each only when stated, `/Alt`, `/ActualText`, `/E`, `/Lang` and `/T`,
  written as text strings for the declared version.
  `PageBuilder::tagged_with(&Tag, …)` is the closure form;
  `open_tag(&Tag)`/`close_tag()` the explicit one, which spans drawing calls
  and pages — an element open when its page is pushed is closed there and
  reopened on the next page begun under a key of the builder's own, and the
  halves merge at `finish` the way `tagged_keyed`'s always have. An element
  that draws nothing is kept when it carries a property or
  `Tag::keep_empty()`, and dropped when it says nothing, which is what the
  first writer did with every empty element. `DocumentBuilder::set_language`
  writes the catalog's `/Lang`. The page-level nesting cap is one below the
  reader's, because `/Document` is a level of its own; it used to equal the
  reader's, and the deepest element's text came back orphaned.
- **`/RoleMap`.** `DocumentBuilder::map_role` maps a custom type to a
  standard one, possibly through another custom type, and refuses to remap a
  standard type (ISO 14289-1 7.1), an identity, a second target and a loop.
  The standard list moved from the PDF/A validator into the writer's crate so
  the two cannot disagree about what "standard" means.
- **`/Link` with its `/OBJR`.** A `link` call made while an element is open
  makes the annotation a content item of it; `link_for(key, …)` does so by
  `tagged_keyed`'s key, for links measured after drawing. `finish` folds the
  document tree *before* writing the pages — merging allocates nothing, so
  documents without such a link are numbered exactly as before — because an
  annotation in the tree needs its own `/StructParent` key, after every
  page's, whose `/ParentTree` value is a reference to its element rather than
  an array. Writing it found that `close_marked`'s take-back of an empty
  sequence removed the element's *last* kid, which after a `link` call is the
  annotation's; it now removes the kid that named the id.
- **Table attributes and identifiers.** `Tag::id` writes `/ID`, claimed in
  reading order before an element's kids are written so the first element
  to carry one keeps it, and `finish` writes the `/IDTree` through the name
  tree writer the named destinations use. `Tag::table(TableAttributes)`
  writes one attribute object owned by `/Table` directly in `/A`. The reader
  reads `/A` (a dictionary, a stream, or an array with revision numbers) for
  the `/Table` owner into the same `TableAttributes` type, and `/C` classes
  are not read.
- **The EPUB's pictures and languages.** The structure emission moved out of
  `epub/paint.rs` into `epub/tagging.rs`. `<img alt>` is a `/Figure`'s
  `/Alt`, an empty `alt` an artifact; a picture is drawn in painting order
  and placed in reading order by a position recovered from the element tree,
  since the layout stamps none on a replaced box, with the paragraph around
  it split by `PageBuilder::continue_at` where the picture falls between two
  of its runs. `dc:language` is the catalog's `/Lang`, `xml:lang`/`lang` an
  element's, and the elements at a chapter's top carry the chapter's own
  language where it differs from the book's.
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
  drawn and written as itself; `sub` stopped being the non-standard `/Sub`.
- **The census over this engine's own output.** `pdfua.rs` runs milestone 5's
  rules over an EPUB this engine converted and over a document built with the
  tagging API, and asserts exactly what still fires: `no-pdfuaid-part` (no
  PDF/UA claim is written, and none should be until the PDF/UA design's
  ledger says so) and `font-not-embedded` (both are set in the unembedded
  standard 14). Before this row the EPUB output also stated no language.

What the row leaves, each named in a refusal table rather than absent: a
link wrapped across lines is one annotation per rectangle rather than ISO
32000-2 erratum 133's one annotation with `/QuadPoints`
([content-and-text](../features/content-and-text.md)); attributes reached
through `/C` and the `/ClassMap` are not read (the same); an empty EPUB table
cell is not in the tree ([epub](../features/epub.md)).

## Dependencies

- `crates/tinker-pdf-cos/src/trees.rs` — `number_tree` for `/ParentTree`;
  `limits`, `WarningKind` for bounds and provenance. No new crate: structure
  is document semantics and lives in the facade (rulings 8 and 11).
- `crates/tinker-pdf-content` — `Device::begin_marked_content`,
  `FontSource`, `TextDevice`/`TextPage` (rulings 2, 7, 10).
- Corpus harness — `xtask/src/corpus.rs`, `ratchet.rs`, `tools/tpdf`
  probe records; the fetched veraPDF corpus pinned in `corpus/corpora.lock`.
- Milestone 6 — `DocumentBuilder` and the write path in `tinker-pdf-cos`;
  the strict structural validator beside it (ruling 13).

## Risks

| Risk | Mitigation |
|---|---|
| Adversarial structure trees: `/K` cycles, role-map loops, thousand-deep nesting | The `trees.rs` discipline — visited set, depth and count caps, typed truncation warnings — plus fuzz coverage from milestone 1; a refused subtree degrades, never panics (rulings 1, 2) |
| Inline property-list reassembly misreads flattened tokens | The scan is bounded, reads plain values only, and its failure mode is "no MCID, scope visible" — asserted by the malformed-list tests in milestone 2, the direction already ruled for `/OC` |
| `/ParentTree` and the `/K` walk disagree in real files | The `/K` walk is authoritative; the disagreement is a counted, provenance-carrying warning (ruling 10) and a ratcheted corpus number, so the wild's rate is measured before anything depends on it |
| Structured view drifts from flat extraction | One `TextDevice`, one `TextPage`; milestone 3's exit pins `plain_text()` byte-identical across the fingerprint suite |
| Corpus structure counts are noisy across runs | Counts are deterministic per file (ruling 4 discipline; no wall-clock inputs) and compared as ratchet bars, not per-file assertions — the same mechanism that keeps the render bars stable |
| veraPDF corpus annotations cover clauses this design does not implement | Milestone 5 diffs only the implemented clauses; anything else is out of scope until `design/pdfa.md` builds the general conformance machinery |
