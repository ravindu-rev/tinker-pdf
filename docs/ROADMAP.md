# Roadmap

The goal, stated as a capability target: **every document a real producer
emits opens, renders provably the way the world renders it, round-trips,
signs, conforms — on every target, deterministically, with no unsafe code
and no silent failure.** The feature docs record what already holds; this
file orders what does not yet, by evidence.

Every item carries its **evidence** (a measured number or a named refusal),
its **exit criterion** (a test or CI job that runs — a roadmap item without
one is not done when the code lands, it is done when the check goes green),
and a **size band**: S ≈ 0.5 engine-months, M ≈ 1–2, L ≈ 2–4, XL ≈ 5–8.
L and XL items have a design doc in [design/](design/); S and M items live
here alone. Scheduling within a tier follows corpus hit-rate evidence
(ruling 3, [rulings.md](rulings.md)), not interest.

## Tier 1 — prove correctness

These come before any new feature. The suite is 2 963 tests proving the
engine agrees with itself, and ruling 13 says that is the only kind of proof
this repository will have. That raises the bar on what those tests must be
rather than lowering it: answers computable in closed form, bitstreams
transcribed from the standards' own annexes, published conformance data, and
thousands of documents nobody here authored.

- **Resampling is not coherent across scale on strip-built scans.** What is
  left of the `dpi` relation's eight failures after image edges, magnification
  and conflation were each measured and fixed
  ([design/image-edges.md](design/image-edges.md),
  [features/rasterizer.md](features/rasterizer.md)). Three of the eight now
  hold outright; `pclm-in.pdf` improved from 17.3 % of pixels disagreeing to
  15.9 %, against a 2 % budget, and kept `rotate` and `crop`. The residual is
  neither edges nor conflation — both were isolated and measured out, and a
  render and the box-filtered render at twice the scale now agree to 0.00 % on
  a synthetic source at every ratio tried — so what remains is resampling
  itself on real scans, and it wants its own evidence before it wants a patch.
  Carried with it: the two `inline-images-ii-*` files break `rotate` at 1.7 %
  against a 1 % budget, because an anti-aliased image edge at a fractional
  offset does not transpose exactly where a quantised one did. That is a
  budget to revisit or an artefact to remove, and it is stated rather than
  absorbed. Evidence: `tpdf probe --dpi 72` over the eight qpdf files, August
  2026, per file in [design/image-edges.md](design/image-edges.md). Exit: the
  `dpi` row holds on the PCLM pair, and `rotate` holds on all eight. (M)

## Tier 2 — close the named refusals, by measured reachability

Every item that stood here when the tier was written has landed and left under
this file's own rule, the last of them JBIG2 refinement coding (clause 6.3): `SDREFAGG`,
`SBREFINE` and segment types 40, 42 and 43 all decode, with both of 6.3.5.3's
context templates and 6.3.5.6's typical prediction. The evidence is the pdf.js
corpus coding one 399 by 400 picture a dozen ways — the encodings that do not
refine are ground truth for the ten that do, and all ten reproduce it with **0
pixels different**, checked by
[`jbig2_refinement.rs`](../crates/tinker-pdf/tests/jbig2_refinement.rs).
T.88 Annex H's page 3 decodes into its own refined text with no warning.

The route there is written up in
[design/jbig2-symbol-text.md](design/jbig2-symbol-text.md) and is worth reading
before the next item of this shape is scheduled: 6.3.5.3's figures were never
obtained, and did not need to be. A context index only labels an adaptive state
slot, so the standard's bit order is unobservable and only the *set* of
positions is a fact about the format. What that argument does not buy is the
right to guess the set: five candidate templates agreed with each other on
Annex H's thirty-six-decision fixture and were all wrong, and only a whole
picture coded both ways told them apart.

Closing it put one item back — refinement over the Huffman road, measured at
11 segments in 10 files — and that item has since closed too: 6.4.11's
envelope, tables B.14 and B.15, and 6.5.8.2 over Huffman all decode, held by
15 corpus files at 0 pixels different in
[`jbig2_refinement.rs`](../crates/tinker-pdf/tests/jbig2_refinement.rs).
**The symbol lineage is now complete**: arithmetic and Huffman, with and
without refinement, in either combination.

Two things are worth carrying forward rather than filing away, both written up
in [design/jbig2-symbol-text.md](design/jbig2-symbol-text.md):

- **A reconstruction is only as good as the line a fixture exercises.** B.14
  and B.15 are reconstructed, both files that reach them code every delta as
  zero, and counted injection shows that changing any other line in either
  table breaks nothing. A non-zero delta is therefore refused rather than
  decoded through the unverified part — the guard lifts when a fixture
  exercises the rest.
- **Three separate defects in this lineage all presented as a wrong context
  template**, and all three were found by a picture rather than by reasoning.
  A decoder with internal state a header cannot check wants a fixture that
  renders a whole page.

Refusals still standing in this lineage, each a named refusal rather than a
scheduled item — their reachability is below what ruling 3 has scheduled
before: halftone regions and pattern dictionaries (16 corpus files — a third
lineage, [features/filters.md](features/filters.md)), transposed text regions
(4), and type 53 custom code tables (6, five of which are refining Huffman
regions).

## Tier 3 — capabilities absent today

Ordered by leverage, not size.

- **Digital signatures.** Nothing verifies or produces one. Reading:
  `/ByteRange` + CMS/PKCS#7 verification (12.8), X.509 parsing, RSA and
  ECDSA verify — hand-rolled, verify-only, under the same rules as the
  rest of the crypto. Writing: sign on incremental update — the
  byte-identical prefix a signature needs already exists and is tested.
  `/DocMDP` and modification detection follow. Ruling 13 costs this item
  its continuous interop check: nothing in CI may ask another program
  whether a signature is acceptable, so a signature everything in-tree
  accepts may still be rejected by real validators, and the design doc says
  so. Exit: verify a corpus of signed documents; the published CAVP and RFC
  test vectors gate the primitives; interop is a dated, recorded, one-time
  measurement outside CI. (XL, [design/signatures.md](design/signatures.md))
- **Text shaping — a non-goal, overturned.** The docs long stated shaping
  as permanent non-goal, and for *rendering existing PDFs* the reasoning
  holds: the producer positioned every glyph. It fails wherever this
  engine is the producer — `DocumentBuilder`, form-fill appearances, EPUB
  layout — none of which can set Arabic, Indic or ligature-dependent text
  correctly today. Overturned here, staged: a shaping leaf crate
  (OpenType GSUB/GPOS, bidi; bytes and a face in, positioned glyphs out),
  consumed first by `tinker-pdf-layout`, then creation, then forms. Exit:
  an Arabic EPUB paginates legibly; a shaped glyph run round-trips through
  `DocumentBuilder`. (XL, [design/shaping.md](design/shaping.md))
- **Tagged PDF and accessibility.** No structure tree, `/StructParents`,
  `/MarkInfo` or role map is read anywhere. Reading order, alt text,
  PDF/UA checks — the veraPDF corpus already carries the cases. Exit:
  structure-aware text extraction; PDF/UA rows measured. (L,
  [design/tagged-pdf.md](design/tagged-pdf.md))
- **PDF/A validation and writing.** No `/OutputIntent` handling and no
  conformance machinery; the 2 907-file veraPDF corpus — the largest in
  the harness — is used purely as a never-crash bar. It graduates to a
  conformance bar. Exit: validation verdicts match veraPDF's own
  pass/fail corpus annotations; the writer gains a PDF/A profile. (L–XL,
  [design/pdfa.md](design/pdfa.md))
- **Streaming open.** The contract today is whole-file-in-memory
  (`Arc<[u8]>`), stated where `Document::open` is declared; memory-mapping
  is the caller's business on native. A range-request-shaped reader —
  open, show page one, fetch the rest — has its spec-complete counterpart
  in-tree already: the writer produces Annex F linearized files and reads
  back its own hint tables. Exit: first page rendered from a byte-range
  source without the tail. (L,
  [design/streaming-open.md](design/streaming-open.md))
- **Bindings write surface.** The four bindings project reading only.
  `DocumentEditor` and `DocumentBuilder` cross the C ABI and the three
  bindings under ruling 11 — the facade shape is already the design. Exit:
  fill-and-save and build-a-document demonstrated from all four. (M–L,
  [design/bindings-write.md](design/bindings-write.md))
- **Public-key encryption.** The `Adobe.PubSec` handler family is absent;
  password handlers R2–R6 are complete. (M)
- **CFF subsetting.** Embedding subsets TrueType only; a CFF face embeds
  whole. Charstring subsetting with subroutine renumbering. (M)

Decision items, not commitments: **OCR** (if ever, as a host seam like
`FontProvider`, not an in-engine engine) and **container writing** (CBZ,
XPS and EPUB are read-only conversions today). Named permanent non-goal:
**XFA** — removed in ISO 32000-2; stated so it is a decision rather than
an omission.

## Tier 4 — container depth

Debts each container format recorded about itself, in its feature doc's
refusal table.

- **EPUB** ([features/epub.md](features/epub.md)): a fixed-layout book
  from a real producer; a real producer's font through the `@font-face`
  path (WOFF/WOFF2 are refused by name); table-row and flex-line
  fragmentation (staged, not built); the pinned float reading-order
  defect; multi-column; non-static `position`; `min`/`max` sizing;
  `vertical-align`; SVG content documents; CSS `@layer`; pseudo-classes
  and pseudo-elements (parsed, never matched / no box generated).
- **XPS** ([features/xps.md](features/xps.md)): `VisualBrush`; TIFF and
  JPEG XR decoders; remote resource-dictionary parts; `IsSideways` and
  odd `BidiLevel`; `OpacityMask`; and a corpus from more than one
  producer — every committed package is from a single vendor's two
  serialisers, and the doc says so.
- **CBZ** ([features/cbz.md](features/cbz.md)): open archives real
  archivers wrote (none has been — every fixture is hand-built, and the
  doc says so); CBR/CB7/CBT, sized honestly — CBR means a hand-rolled RAR
  decoder; `ComicInfo.xml` metadata.
- **Forms** ([features/forms.md](features/forms.md)): an execution policy
  for the surfaced-but-never-run keystroke, validate and document-level
  scripts; whether a format action's display string may ever reach `/V`.

## How this file changes

An item leaves this file when its exit criterion is green, and its feature
doc's refusal table loses the matching row in the same commit. An item
enters with evidence attached — a corpus number, a named refusal, an owed
assertion — or it does not enter. Design docs in [design/](design/) carry
scope, non-goals, design, milestones with concrete exit criteria, and
risks, in that order.
