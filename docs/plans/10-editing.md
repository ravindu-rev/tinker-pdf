# Phase 10 — Editing

When this phase is done, tinker-pdf mutates documents: full annotation CRUD with
Acrobat-grade synthesized appearances, page surgery including cross-document merge,
forensically sound redaction, flatten-to-content, and font subsetting on write. Every
mutation flows through one exclusive `DocumentEditor` whose commit produces a change-set
the incremental writer ([09-writing](09-writing.md)) consumes — there is no second write
path. The shape follows from Tinker's MuPDF experience: every editing feature Tinker
planned ended up routed through unsafe `mupdf-sys` shims for things the bindings never
exposed (annotation dates, stamp images, `/IRT`, `pdf_bake_document`, `pdf_subset_fonts`,
per-glyph `pdf_filter_page_contents` — the whole "Reported" table of Tinker's
`mupdf-limitations.md`). Here each of those is a first-class API, designed before the
first caller instead of reverse-engineered around a binding gap. The phase runs
post-integration by design ([PLAN.md](../PLAN.md) lane table): Checkpoint B does not need
it, and it needs almost everything Checkpoint B built.

## Scope

- **Exclusive editor over the COS store** — copy-on-write overlay on the object store
  from [01-cos-and-object-model](01-cos-and-object-model.md); open readers keep their
  snapshot; `commit` yields the changed-set the incremental writer serializes as an
  update section (7.5.6); explicit not-`Sync` mutation model.
- **Annotations** (12.5) — create/modify/delete for Highlight, Underline, StrikeOut,
  Squiggly (12.5.6.10, `/QuadPoints`), Text (12.5.6.4 sticky notes, icon names), FreeText
  (12.5.6.6, `/DA`/`/Q`), Ink (12.5.6.13, `/InkList`), Square/Circle (12.5.6.8, `/IC`,
  `/RD`), Line (12.5.6.7, `/L`, `/LE` ending styles), Polygon/PolyLine (12.5.6.9,
  `/Vertices`), Stamp (12.5.6.12, standard names *and* image stamps), Popup (12.5.6.14),
  Link (12.5.6.5). Markup common entries throughout: `/T`, `/CreationDate`, `/M` (7.9.4
  date format), `/NM` names, `/IRT`+`/RT` reply threading (12.5.6.2).
- **Appearance-stream synthesis** — generated `/AP` `/N` Form XObjects per type (12.5.5):
  highlight through `/BM /Multiply` in a non-isolated transparency group, FreeText `/DA`
  parsing plus line-wrap layout, ink smoothing hooks, all Table 176 line endings, cloudy
  borders (`/BE`, Table 167). The foreign-AP preservation rule: an appearance we did not
  author is never regenerated.
- **Page operations** — insert/delete/move/reorder over the page tree (7.7.3) with
  destination, outline (12.3.3), and link fixups; inherited-attribute flattening
  (Table 30) before any move; cross-document merge via resource grafting with a
  per-source dedup map (graft-map equivalent); split into new documents.
- **Redaction** — Redact annotations (12.5.6.23) as marks; apply removes content: quads
  drive per-glyph removal through the `ContentFilter` seam built in
  [06-content-and-text](06-content-and-text.md), image scrub (pixel-blank or remove
  whole), recursion into Form XObjects with copy-on-shared instancing, annotation
  cleanup. The acceptance test is Tinker's: save, decompress every stream, assert the
  needle bytes are absent anywhere in the file.
- **Flatten/bake** — annotation and form-widget appearances composed into page content
  via the 12.5.5 `/BBox`-`/Matrix`-to-`/Rect` mapping, then the annotations deleted.
- **Font subsetting on write** — used-glyph collection over all content streams,
  TrueType `glyf`/`loca`/`cmap` rebuild, CFF charstring subset, `/Widths` and `/W`
  rewrite (9.6.6.2, 9.7.4.3), subset-tag prefixes (9.6.4).

## Non-goals

- **Form field logic** — field creation, value setting, calculation, widget synthesis
  live in [11-forms](11-forms.md). This phase's flatten consumes widget `/AP` streams as
  opaque appearances; it never computes one.
- **In-place text editing, text boxes, object move/resize** — Tinker builds these in its
  own core by composing this phase's primitives (redaction as the removal half, content
  append from [12-creation](12-creation.md) as the insertion half), exactly as its
  `plans/04` designed them against MuPDF. The engine ships the primitives, not the
  editor UX.
- **XFDF interchange** — Tinker's hand-rolled quick-xml implementation survives the
  engine swap untouched; it talks to this phase's annotation CRUD and needs nothing else.
- **Optimization** — garbage collection and object-stream packing are the writer's
  ([09-writing](09-writing.md)); image downsampling and whole-document font subsetting
  as a *shrink* pass are an optimizer concern layered on this phase's subsetter later.
- **Annotation subtypes we do not author** — Sound, Movie, Screen, 3D, FileAttachment,
  Caret, Watermark, Widget. Preserved byte-exact on load/save, deletable, never created.
- **FreeText rich text (`/RC`) and callouts (`/CL`)** — plain text and `/Q` alignment
  only; `/RC` is written mirroring the plain contents so Acrobat does not resurrect
  stale rich text. Documented limit, revisited on demand.
- **Redaction `/OverlayText`** — the applied region is filled with `/IC` or left blank;
  overlay text layout is deferred (it is FreeText layout reuse when wanted).
- **Digital signatures** — signing rides the same incremental writer but is its own
  effort with Tinker's security module; nothing here may invalidate its byte-range
  assumptions, which is one more reason all writes go through 09's single path.

## Design

### The editor: copy-on-write, one writer, live readers

The object store from [01-cos-and-object-model](01-cos-and-object-model.md) is an
immutable snapshot behind an `Arc`. The editor is an overlay:

```rust
pub struct DocumentEditor {
    base: Arc<Snapshot>,
    overlay: HashMap<ObjNum, Slot>, // Slot = Modified(Object) | New(Object) | Deleted
    next_obj: ObjNum,
    diagnostics: Vec<EditWarning>,
    // deliberately !Sync: interior state is single-writer by construction
}

impl Document {
    /// At most one live editor per document. A second call while one exists
    /// fails with EditorBusy rather than deadlocking or aliasing.
    pub fn edit(&self) -> Result<DocumentEditor, EditorBusy>;
}

impl DocumentEditor {
    /// Consumes the editor. The ChangeSet is exactly what the incremental
    /// writer appends: modified/new objects, freed numbers, trailer deltas.
    pub fn commit(self) -> (ChangeSet, Arc<Snapshot>);
    pub fn abort(self);
}
```

Reads during editing resolve overlay-first, then base — the editor sees its own writes,
readers on the base `Arc` see nothing until they opt into the post-commit snapshot.
That is the whole concurrency story, stated as types: `DocumentEditor` is `Send` but
deliberately not `Sync`. One mutator, moved freely between threads, never shared; the
`edit()` gate is an atomic flag on the document. Tinker's document-actor model maps onto
this exactly (the actor thread owns the editor), and nothing subtler is expressible, so
nothing subtler can be wrong.

`commit` does not write bytes. It returns the `ChangeSet` and the new snapshot; the
caller hands the change-set to 09's incremental writer (appended xref section or stream
per 7.5.6, `/Prev` chained) or to its full-rewrite path when the host asked for a clean
save. Undo at the Tinker layer is what it always was — snapshots — but now a snapshot is
an `Arc` clone, not a serialized buffer, because the base is immutable.

### Annotations: dictionary model

```rust
pub enum AnnotKind {
    Highlight, Underline, StrikeOut, Squiggly,
    Text, FreeText, Ink, Square, Circle, Line,
    Polygon, PolyLine, Stamp, Popup, Link,
}

impl DocumentEditor {
    pub fn annot_create(&mut self, page: PageIdx, spec: AnnotSpec) -> Result<AnnotRef, EditError>;
    pub fn annot_patch(&mut self, annot: AnnotRef, patch: AnnotPatch) -> Result<(), EditError>;
    pub fn annot_delete(&mut self, annot: AnnotRef, replies: ReplyPolicy) -> Result<(), EditError>;
    pub fn annot_reply(&mut self, parent: AnnotRef, spec: ReplySpec) -> Result<AnnotRef, EditError>;
}
```

`AnnotSpec`/`AnnotPatch` carry the typed per-kind payloads (quads, ink strokes,
vertices, line endpoints and endings, stamp name or image, link destination or URI).
Every create writes `/NM` (caller-supplied or generated through the `EntropySource`
seam from [03-encryption](03-encryption.md) — the engine still links no RNG),
`/CreationDate` and `/M` in 7.9.4 `D:` format from a caller-supplied timestamp — the
engine links no clock either; wasm32-unknown-unknown has no ambient time, and the
first-class-wasm rule from [PLAN.md](../PLAN.md) decides this the same way it decided
entropy. Patches bump `/M` only.

Replies are Text annotations with `/IRT` → parent and `/RT /R` (12.5.6.2). The engine
stores and resolves the graph; thread assembly (root plus replies ordered by
`/CreationDate`) is a read-side helper so Tinker's sidebar does not reimplement date
parsing. `annot_delete` on a thread root takes a `ReplyPolicy`: cascade or reparent —
the two behaviors Tinker's plan already committed to. `Popup` entries are maintained
automatically for markup kinds that want one: create wires `/Popup`/`/Parent` both
directions, delete removes the pair.

Everything MuPDF's bindings made Tinker shim — date setters, stamp images, `/IRT`
access — is native surface here. There is no shim layer to design because there is no
binding boundary.

### Appearance synthesis: the Acrobat-compat hard part

A conforming reader may regenerate appearances, but real interchange means the `/AP` we
write is what Acrobat, Preview, and pdfium display without needing to. Each kind gets a
generator producing a Form XObject (`/BBox`, `/Matrix`, `/Resources`, content):

- **Text markup.** One quad-aligned fill per `/QuadPoints` quad. Highlight draws through
  an `ExtGState` with `/BM /Multiply` inside a non-isolated `/Group /Transparency` form
  — non-isolated is the load-bearing word: Multiply must read the *page* backdrop or
  the highlight paints opaque over the text (11.3.5, 11.6.6). Underline/StrikeOut/
  Squiggly are stroked lines at quad-derived offsets; squiggly is a fixed-period sine
  approximated with beziers.
- **FreeText.** Parse `/DA` (12.7.3.3 grammar: `Tf` plus a color operator), resolve the
  named font against `/DR` then our bundled base-14 metrics from
  [05-fonts](05-fonts.md), lay out with greedy line wrap on advance widths, honor `/Q`.
  Fonts we place are registered in the form's `/Resources` and become subsetting input.
- **Ink.** `/InkList` polylines stroked with round joins/caps. A smoothing hook —
  `fn smooth(&[Point]) -> Cow<[Point]>` — lets the host (Tinker uses centripetal
  Catmull-Rom) prettify the *appearance* while `/InkList` keeps the raw committed
  points; the file stays interoperable, the pixels stay pretty.
- **Line.** Endpoint geometry plus all Table 176 endings (OpenArrow, ClosedArrow,
  Circle, Diamond, Square, Butt, Slash, ROpenArrow, RClosedArrow, None), each a small
  path generator parameterized on border width, with `/LL`/`/LLE` leader lines.
- **Square/Circle/Polygon/PolyLine.** Stroke `/C`, fill `/IC`, dash from `/BS`, `/RD`
  inset. Cloudy border (`/BE /S /C`, intensity `/I` 0–2): the boundary is replaced by a
  sequence of overlapping half-circle arcs whose radius scales with `/I` — matched
  against Acrobat renders perceptually, not analytically; Adobe never specified the
  curve.
- **Stamp.** Standard names (Approved, Draft, Confidential, …) from one bundled
  vector template set; image stamps embed the decoded image as an Image XObject drawn
  to fit `/Rect` preserving aspect.
- **Link/Popup.** No `/N` needed (links have no appearance; popups are viewer-drawn);
  we write border dictionaries honestly (`/Border [0 0 0]` default — invisible, which
  is what every tool ships).

Two rules keep fidelity honest:

- **Foreign-AP preservation.** We never regenerate an `/AP` we did not author. A patch
  that only moves or resizes rewrites `/Rect` and lets the 12.5.5 BBox→Rect mapping
  scale the existing appearance — byte-identical stream, new placement. Only a patch
  that invalidates the appearance itself (color, contents, geometry *shape*) triggers
  synthesis, and then the regeneration is recorded in the change-set diagnostics so a
  host can warn. This is Tinker's plan-02 rule, promoted from convention to invariant.
- **`/AP` is always written on creation.** ISO 32000-1 12.5.2 requires it for
  practically everything, Acrobat is unforgiving without it, and synthesizing at
  create-time means our render path and everyone else's agree by construction.

### Page operations

Insert/delete/move/reorder are `/Kids`/`/Count` surgery on the page tree (7.7.3.2) with
`/Parent` maintenance, plus the fixups that make the result *mean* the same thing:

- **Inherited-attribute flattening.** Before a page detaches from its ancestry (move,
  merge, split), the Table 30 inheritables — `/Resources`, `/MediaBox`, `/CropBox`,
  `/Rotate` — are materialized onto the page dictionary. A page must render identically
  under any parent.
- **Destination fixups.** Explicit destinations hold page *references*, so reorder is
  free. Delete walks every destination site — outline items (`/Dest` or GoTo `/A`,
  12.3.3/12.6.4.2), the `/Dests` name tree (7.7.4) and legacy `/Dests` dictionary,
  `/OpenAction`, and Link annotations — and removes destinations that point at dead
  pages, keeping the carrying item and emitting a structured warning. Retargeting to a
  neighbor guesses intent; dropping is honest and matches Acrobat's observable
  behavior.
- **Merge.** `graft(src: &Document, pages: Selection) -> …` deep-copies object graphs
  through a per-source-document map `(src_obj, src_gen) → dst_obj` — the graft-map
  equivalent — so a resource referenced by fifty source pages lands once, and cyclic
  graphs (page `/Parent`, `/Popup`↔`/Parent`) terminate. `/Parent` edges are cut and
  rewired to the destination tree, never grafted. Outlines concatenate with remapped
  destinations. Deliberately *not* attempted, mirroring Tinker plan-03's documented
  limits: AcroForm field-name collision renaming (tracked for
  [11-forms](11-forms.md)) and structure-tree grafting — tagged PDF structure is
  dropped from merged output with a warning. Cross-source byte-level dedup (two files
  embedding the same font) is an optimizer job, not a graft job.
- **Split.** The same grafting machinery pointed at a fresh document; nothing new.

### Redaction

Marks are ordinary Redact annotations (12.5.6.23) so they survive save-as-draft in any
viewer. Apply is the destructive half, built on the `ContentFilter` seam
[06-content-and-text](06-content-and-text.md) exposes — the interpreter re-emits a
content stream, giving per-operation callbacks with full graphics state:

```rust
pub enum ImageVerdict { Keep, Remove, Scrub(Vec<Rect>) } // rects in image space

pub trait ContentFilter {
    fn glyph(&mut self, g: &GlyphInstance<'_>) -> bool;      // keep?
    fn image(&mut self, im: &ImageUse<'_>) -> ImageVerdict;
    fn path(&mut self, p: &PathUse<'_>) -> bool;             // line-art option
}
```

- **Per-glyph removal.** A dropped glyph inside `Tj`/`TJ`/`'`/`"` is replaced by a `TJ`
  adjustment reproducing its full displacement — `w0·Tfs/1000 + Tc` (plus `Tw` for a
  single-byte code 32), converted back into `TJ` thousandths — so retained glyphs do
  not move. Show strings split as needed; the quad test uses the glyph's rendered
  bbox under the full CTM, so rotated pages and CropBox offsets need no special case.
- **Form XObject recursion.** Filtering recurses through `Do` of Form XObjects. A
  shared form that filters differently at different use sites is instanced — cloned to
  a new object for the affected site — so redacting page 3 never edits page 7. Inline
  images (`BI`/`ID`/`EI`) are scrubbed in place.
- **Image scrub.** The redact region maps into image space via the inverse CTM. Fully
  covered images have their `Do` removed and the XObject freed if unreferenced.
  Partially covered images are decoded ([02-filters](02-filters.md)), the region
  blanked, and re-encoded as FlateDecode regardless of source filter — re-encoding
  JPEG invites generation loss and CCITT/JBIG2 re-encoders are not worth building for
  this; `/SMask` is scrubbed in the same region.
- **Line art.** Optional (`strip_vector: bool`, default off): drop path-painting ops
  whose stroked/filled extent intersects the quads. Off by default because borders and
  table rules crossing a redaction are usually wanted.
- **Annotation cleanup.** Markup annotations lose intersecting quads (fully covered ⇒
  deleted); Link annotations intersecting the region are deleted; applied Redact
  annotations are themselves removed and the region optionally filled with `/IC`.

The acceptance test is engine-independent and inherited verbatim from Tinker's
plans/04: save, decompress **every** stream — page content, Form XObjects, object
streams — and assert the needle bytes appear nowhere. Overlay-style fake redaction
fails this by construction. Under-removal that the filter cannot express (Type3 glyph
procedures painting outside their advance, text rendered as vector paths) is exactly
what the audit exists to catch: audit failure surfaces as `RedactIncomplete` with the
offending stream, never a silent success.

### Flatten/bake

Per page, per annotation: skip `/F` Hidden and NoView (Table 165; default policy bakes
what prints, host-overridable), resolve `/AP` `/N` through `/AS` when the appearance is
a state subdictionary, compute the 12.5.5 mapping (transform `/BBox` by `/Matrix`,
bound it, derive the affine map onto `/Rect`), then append `q <cm> /FlatN Do Q` to the
page content with the form registered under a fresh resource name. Then delete the
annotation. Widgets flatten identically — their current `/AP` already reflects field
state, and [11-forms](11-forms.md) owns making that true. The original page content is
wrapped `q … Q` first so annotation appearances cannot inherit a dangling graphics
state. Structure-tree references to baked annotations (`/StructParents`) go stale;
warned, not repaired — same honesty Tinker's plans committed to. This is
`pdf_bake_document` as an ordinary editor operation.

### Font subsetting on write

Runs at commit for fonts this phase embedded (FreeText, future creation-phase text) and
on request for any embedded font. Pass one collects usage: walk every content stream
(and Form XObject) recording `(font, code)` pairs via the 06 interpreter, resolve to
glyph ids through [05-fonts](05-fonts.md)'s cmap/encoding machinery, close over
composite references (TrueType component glyphs, CFF `seac` accents).

The core decision: **glyph ids are never renumbered.** Renumbering would require
rewriting every show string and `/CIDToGIDMap` in the document — high-risk surgery for
marginal extra bytes. Instead unused glyphs are hollowed:

- **TrueType.** `glyf` entries for unused gids become zero-length (`loca` collapses),
  which is where the bytes are; `hmtx` retained (metrics must survive); `cmap` trimmed
  to used codes for simple fonts, dropped for CIDFontType2 with an explicit
  `/CIDToGIDMap`; `head`/`hhea`/`maxp` rewritten consistently; other tables dropped
  unless load-bearing (`cvt `/`fpgm`/`prep` kept when any retained glyph is hinted).
- **CFF.** Unused charstrings replaced by a minimal `endchar` stub; charset and
  FDSelect/FDArray (CID-keyed) kept aligned with the stable gid space. Subroutines are
  kept as-is in v1 — GC requires renumbering every call site and charstring bytes
  dominate; noted as a later shrink, not a correctness item.
- **Type1.** Not subset. eexec charstring surgery is poor return for a format that is
  rare in *newly embedded* fonts; Type1 fonts pass through whole, documented.

`/Widths`/`/FirstChar`/`/LastChar` (9.6.6.2) and `/W`/`/DW` (9.7.4.3) are rewritten to
cover exactly the used codes/CIDs; `/FontDescriptor` gets the new `FontFile2`/
`FontFile3`. The subset tag (9.6.4, six uppercase letters plus `+`) is derived from a
hash of the used-glyph set and font bytes — deterministic, so identical inputs produce
identical files and the test suite can byte-compare. This kills the `pdf_subset_fonts`
gap without inheriting its shape.

### Error and leniency policy

Editing a malformed document must not be the thing that breaks it further. The editor
operates on the repaired object model from 01 and inherits its diagnostics channel;
every fixup this phase performs (dropped destination, instanced XObject, regenerated
foreign AP, skipped Type1 subset, structure-tree orphan) emits a structured warning.
Operations that cannot complete safely fail the single operation with a typed
`EditError` and leave the overlay consistent — a failed `annot_create` writes nothing.
The one hard rule: `commit` either yields a change-set that round-trips (the writer's
own validation) or the whole edit session aborts; the engine never emits a file it
cannot itself reopen.

## Milestones

Every milestone below carries the same standing gate in addition to its own criteria:
output passes `qpdf --check` and veraPDF's parser (both as CI subprocess oracles, per
[14-testing-and-corpora](14-testing-and-corpora.md)), and `pdfcmp` render diffs
before/after stay within per-fixture budgets except where the operation intends change.

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | `DocumentEditor`: COW overlay, `edit()` exclusivity, `ChangeSet`, incremental-write wiring | Readers on the pre-edit snapshot produce byte-identical output while an edit commits; second `edit()` returns `EditorBusy`; commit → incremental write → reopen sees the change and `/Prev` chain is intact; `abort` leaves the file untouched | M |
| 2 | Annotation dictionary CRUD: all 15 kinds, `/NM`, dates via injected clock, `/IRT` threading, Popup pairing, `ReplyPolicy` | Create-save-reopen golden (JSON annot dump) for every kind; replies added to the Acrobat-authored thread fixture appear threaded in Acrobat; delete-with-cascade and reparent both verified; dates match the injected clock exactly | S |
| 3 | AP synthesis wave 1: text markup (Multiply highlight), Square/Circle, Line + all Table 176 endings, Ink + smoothing hook | `pdfcmp` vs Acrobat-authored appearance goldens per kind within budget; highlight over dark text keeps text legible (Multiply pixel assertion); untouched foreign file saves with every `/AP` stream byte-identical | M |
| 4 | AP synthesis wave 2: FreeText (`/DA` parse + wrap layout), stamps (standard set + image), cloudy borders, `/RD` insets | FreeText renders within perceptual budget of Acrobat's layout on the fixture set; image stamp round-trips through save/reopen/render; cloudy `/I 1` and `/I 2` within budget vs oracle renders | M |
| 5 | Page ops: insert/delete/move/reorder, inherited-attribute flattening, destination/outline/link fixups | After arbitrary reorder+delete scripts on the outline fixture, every surviving outline entry resolves to its intended page; a moved page renders identically pre/post (inheritance materialized); dead-page destinations removed with warnings, `qpdf --check` green | M |
| 6 | Merge + split via graft map | Merging two corpus files opens clean in all three oracle viewers (mutool, pdftoppm, pdfium_test); shared source resources grafted exactly once (object-count assertion); concatenated outline destinations land correctly; split-by-ranges round-trips page-exact | M |
| 7 | Redaction: Redact marks, `ContentFilter` glyph removal, image scrub, XObject instancing, annotation cleanup, byte-absence audit | Ported redact-targets fixture: planted needles absent from every decompressed stream incl. Form XObjects and object streams; retained glyphs pixel-stable outside quads; shared-XObject fixture redacts one page without touching the other; scanned fixture pixel-scrubs; audit failure path raises `RedactIncomplete` on the Type3 adversarial fixture | L |
| 8 | Flatten/bake: annotations + widgets to page content | Flattened output has zero annotations; `pdfcmp` pre/post within budget; Hidden/NoView not baked; `/AS`-stated widget bakes its current state; wrapped `q…Q` survives the unbalanced-graphics-state fixture | S |
| 9 | Font subsetting on write: usage collection, TrueType + CFF hollowing, `/Widths`/`/W` rewrite, deterministic subset tags | CJK fixture font bytes shrink ≥ 60 %; extracted text identical pre/post; render diff clean; subset tag stable across runs (byte-identical repeat builds); composite-glyph and `seac` closure fixtures render all glyphs | M |

Sums to the upper half of the XL band; redaction and the two appearance waves dominate,
and milestone 1 is deliberately front-loaded because every other row stacks on it.

## Dependencies

- **Needs:** [01-cos-and-object-model](01-cos-and-object-model.md) (object store the
  overlay wraps), [09-writing](09-writing.md) (incremental writer consuming
  `ChangeSet`), [06-content-and-text](06-content-and-text.md) (interpreter and the
  `ContentFilter` re-emission seam — built there precisely so this phase would not
  retrofit it), [05-fonts](05-fonts.md) (font parsing for `/DA` resolution, glyph
  closure, subsetting), [02-filters](02-filters.md) (image decode/re-encode for
  scrub), [04-document-semantics](04-document-semantics.md) (page tree, outline,
  destinations, links), [08-rendering-device](08-rendering-device.md) plus `pdfcmp`
  for every visual gate, [03-encryption](03-encryption.md)'s `EntropySource` for
  `/NM`. Runs post-integration: [15-tinker-integration](15-tinker-integration.md) does
  not wait for it.
- **Unblocks:** Tinker's frozen editing roadmap — its plans 02 (annotations), 03 (page
  operations), and 04 (redaction) re-target this API and delete their `mupdf-sys` shim
  designs wholesale; [11-forms](11-forms.md) (widget appearances flatten through this
  phase's bake; field values ride the same editor); [12-creation](12-creation.md)
  (content append into the same `ChangeSet` path); signing work (a stable
  incremental-update discipline is the precondition).

## Risks

| Risk | Mitigation |
| --- | --- |
| Appearance fidelity is judged by Acrobat, whose synthesis is unspecified (cloudy borders, FreeText layout metrics, stamp artwork) | Perceptual budgets against Acrobat-authored goldens, never analytic equality; cloudy/FreeText budgets set per-fixture; where Acrobat's behavior is unknowable we match the oracle renders and document the divergence |
| Regenerating a foreign AP silently changes how another tool's annotation looks | The preservation rule is an invariant, not a habit: move/resize rewrites `/Rect` only; regeneration requires an appearance-invalidating patch and is recorded in diagnostics; a byte-identity test on untouched foreign files guards it in CI |
| Redaction under-removal leaks PII (Type3 procedures, text as vector paths, patterns painting text) | The byte-absence audit is the gate, not the filter's opinion; audit failure is a loud typed error; adversarial fixtures (Type3, outlined text, pattern fills) live in the corpus so the failure mode is exercised, not theoretical |
| Shared-resource aliasing: filtering or editing an object referenced elsewhere corrupts unrelated pages | Reference counting over the snapshot before any in-place stream edit; shared objects are instanced by default; the shared-XObject fixture asserts the untouched page is byte-identical |
| Incremental updates bloat files across long edit sessions | The change-set only carries genuinely modified objects (overlay identity, not dirty-flags); hosts choose full-rewrite saves at natural points; bloat is measured in CI, not guessed |
| Hollowed-glyph subsetting keeps more bytes than renumbering would | Accepted trade: content-stream rewrites are the risk we are buying out of; measured shrink ≥ 60 % on CJK fixtures is the bar, and a renumbering subsetter can layer on later without API change |
| Graft-map merge drops document-level structures users notice (AcroForm collisions, tagged structure) | Same policy Tinker shipped with MuPDF: documented limits with structured warnings, fixtures that prove the limit is known (forms, tagged PDF), field renaming tracked in [11-forms](11-forms.md) |
| The editor API ossifies before Tinker's real editing UX exercises it | Phase runs post-integration by design: Tinker's ported annotation/page-op/redaction tests are written against this API as it lands, and the plans are living documents — API friction gets written down and fixed here, not worked around there |

## As built — the editor, August 2026

The overlay is real and behaves as designed; four things about its *interface*
are not what the sketch above says, and each is written down because a caller
reading the plan would otherwise look for something that does not exist.

1. **There is no `edit()` gate and no `EditorBusy`.**
   `DocumentEditor::new(Arc<CosDocument>)` is the constructor and any number of
   editors may exist over one document. Exclusivity was there to protect a
   mutable store; there is no mutable store to protect — the document is
   immutable behind an `Arc` and every editor accumulates its own overlay — so
   the flag would have guarded nothing. Milestone 1's "second `edit()` returns
   `EditorBusy`" is therefore not a criterion this build can meet, and it is
   struck rather than pretended.
2. **There is no `ChangeSet` and no `commit(self)`.** `save(&self,
   &WriteOptions)` produces the bytes directly, in either mode, and the editor
   survives it — so a caller can save incrementally and then save again. The
   change-set was a handoff to a writer that lives in the same crate and reads
   the overlay itself; the intermediate type would only have copied it.
   `abort(self)` is `drop`.
3. **The overlay is three fields, not one map of `Slot`.** `overlay:
   HashMap<u32, Written>` carries replacements and additions, `deleted:
   HashSet<u32>` carries deletions — a deleted object is *removed* from the
   overlay rather than marked in it, so the two cannot be collapsed — and
   `page_order: Option<Vec<ObjRef>>` carries page surgery, which is `/Kids`
   rewriting deferred to save time rather than object writes made eagerly.
   Together with `next` that is the whole of the editor's mutable state, which
   matters because it is exactly what a rollback restores.
4. **`transaction` is where `commit`/`abort` landed.** Added by PRE-E:
   `transaction(|tx| ...)` restores all four fields on an `Err` return, so "this
   edit applies wholly or not at all" is expressible without a session type.
   Milestone 1's "`abort` leaves the file untouched" is met by it, and the
   reasoning for restoring the object-number counter is recorded in
   [11-forms](11-forms.md) beside the fill API that needed it.

There is no `diagnostics: Vec<EditWarning>` channel either. Operations that
degrade return what they degraded — `Vec<SkippedWidget>` from a fill — rather
than appending to a sink the caller has to remember to drain.

---

Sibling context: [06-content-and-text](06-content-and-text.md) owns the interpreter and
the `ContentFilter` seam; [09-writing](09-writing.md) owns every byte that reaches disk;
[11-forms](11-forms.md) and [12-creation](12-creation.md) build on the same editor.
Master plan, checkpoints, and off-ramps: [PLAN.md](../PLAN.md); rulings in
[99-consistency](99-consistency.md) override this file.
