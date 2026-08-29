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

**Clear.** The last item here was the `dpi` relation on strip-built scans, and
it closed with the residual attributed rather than patched around: the
box-filter pyramid averages source-aligned blocks of two, so its support is
quantised to powers of two and moves with the device scale, while an area
filter over the destination pixel's true source rectangle does not. It now
engages only past 128:1 — the worst downscale the pdfjs corpus asks for is
72:1 — and **all eight files hold all three relations**, with `dpi` at 582 of
582 qpdf files (from 577) and 915 of 944 pdfjs files (from 908).

The `rotate` half closed earlier and differently, by raising a budget against
measured noise rather than by changing the engine: an anti-aliased image edge
at a fractional offset does not transpose to the byte, and the two
`inline-images-ii-*` files sat at 1.7 % against a 1 % line drawn when image
edges were still quantised.

Both are written up in [design/image-edges.md](design/image-edges.md), and the
part worth carrying forward is why it took a real scan to see: at an integer
ratio on a full-canvas draw the pyramid's blocks sit exactly under the
destination pixels and even a pyramid agrees with itself, so a synthetic test
at 2:1 or 4:1 proves nothing about it.
[`a_downscale_agrees_with_itself_at_twice_the_scale`](../crates/tinker-pdf-raster/tests/analytic_sampling.rs)
now states the property in closed form at ratios that are deliberately not
whole.

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

**Digital signatures have left this list.** All nine milestones of
[design/signatures.md](design/signatures.md) are green: signatures are found,
their coverage classified, their CMS and certificates parsed, their digests
and signatures verified against 504 published CAVP and RFC vectors, their
chains walked to caller-supplied anchors, their `/DocMDP` and `/FieldMDP`
honoured, and `DocumentEditor` produces and certifies signatures of its own
with the key held by the caller. The surface is projected through the C ABI
and the .NET binding; [features/signatures.md](features/signatures.md) carries
the refusal table.

Three things are worth carrying forward rather than filing away:

- **The corpus adjudicated more than the specification did.** RFC 5652 §5.4's
  re-encoding — the one rule most likely to be implemented subtly wrong — is
  settled by 19 real signatures from six producers, all of which verify with
  the substitution and none without. And a fifth of signed documents are BER
  rather than the DER ISO 32000 asks for, from two independent producer
  lineages, which no reading of the clause would have predicted.
- **Four documents carry a signature that verifies over bytes they no longer
  have.** veraPDF's permission fixtures share one CMS blob across three files
  of different sizes. Finding them is the feature working, and it is why
  coverage, document digest and signature verification are three answers
  rather than one.
- **What ruling 13 costs here was paid once and written down.** The interop
  measurement ran on 28 August 2026 against OpenSSL 3.5.5, in both directions,
  with negative controls — and the design doc records what it does *not*
  establish as carefully as what it does. It does not run again.

**Public-key encryption has left this list too**, and it is the one item in
this tier whose evidence is worth naming as a warning rather than a result.
`/Adobe.PubSec` reads: the envelope is parsed by `tinker-pdf-pki`, the file key
derived per 7.6.5, and the same decryptor installed that a password produces.
But **zero of the 4 594 corpus files use it** and no tool available here
produces one — qpdf, which is installed, has no public-key support. So the
envelope parsing is held to structures OpenSSL produced, which is real interop,
and the key derivation on top is held only to a second implementation of the
same clause by the same author. That catches a transcription slip and cannot
catch a misreading. [design/pubsec.md](design/pubsec.md) carries it as an open
risk, and it closes the day a real public-key-encrypted document arrives.

**Tagged PDF has left this list.** All six milestones of
[design/tagged-pdf.md](design/tagged-pdf.md) are green. `Document::structure()`
walks `/StructTreeRoot`, `/K`, `/RoleMap` and `/MarkInfo`; MCIDs and the 14.9
properties cross the `Device` seam in both their inline and `/Properties`
forms; `Page::structured_text()` joins them to the *same* `TextPage` the flat
extraction produces, so there is one extractor and not two; the corpus carries
ratchet bars over 717 files with structure trees; PDF/UA is measured; and
`PageBuilder::tagged` writes a tree that reads back with no orphans.

Three things worth carrying forward:

- **A written decision was overturned, and the reason was written first.**
  `interpret.rs` argued against reassembling the inline `<< … >>` property
  list, recording that a defect injection could not make the deleted scan
  change an answer. That was true when only `/OC` was read from the list and
  stopped being true the moment `/MCID` was. Reassembling it also fixed a
  defect the two-token peek carried: the tag sits *before* the `<<`, so
  `/Artifact << /Type /Pagination >> BDC` reported its tag as `Pagination`,
  and the text device extracted running heads as author content.
- **The writer is checked by the reader, and it caught what bytes could not.**
  The first version wrote `/Kids` and `/Parent` on its structure elements —
  the page tree's keys, not 14.7.2 Table 323's `/K` and `/P`. The file parsed
  cleanly, validated cleanly, and had every marked-content id orphaned with no
  error anywhere. A byte-level test would have asserted the bytes it wrote.
- **PDF/UA abstains out loud.** 29 of 239 non-conforming fixtures are caught
  with **zero false alarms**, and 210 are abstentions printed as abstentions.
  Most of ISO 14289 is a judgement about meaning that no reader makes, and a
  census reporting "agreement" over all 434 would have counted those silences
  as successes.

**CFF subsetting has left this list.** A CFF face embeds subset rather than
whole, CID-keyed included — 144 of the 241 corpus `/FontFile3` programs are
`CIDFontType0C`, so FDArray and FDSelect subsetting were in scope from the
first commit rather than deferred. Glyph ids never move, which is why
`/Widths`, `/W`, `/CIDToGIDMap` and `/ToUnicode` need no rewriting. And the
defect found on the way was a ruling 10 violation: `subset()` answering `None`
was swallowed at both call sites and the whole face embedded with **no warning
at all**, observable only as a missing `ABCDEF+` tag. `EmbeddedWhole` and
`SubsetRefusal` now say so.

- **Text shaping — a non-goal, overturned; partly landed.** The docs long
  stated shaping as a permanent non-goal, and for *rendering existing PDFs*
  the reasoning holds: the producer positioned every glyph. It fails
  wherever this engine is the producer. `tinker-pdf-shape` now exists —
  GSUB/GPOS/GDEF, UAX #9 bidi gated on 770 241 `BidiTest` resolutions and
  91 707 `BidiCharacterTest` cases, and 48 of the 77 text-rendering-tests
  cases with the other 29 declined by name. **Not finished**: Arabic
  joining, USE/Indic/SEA, re-shaping, and the three consumers
  (`tinker-pdf-layout`, `DocumentBuilder`, form-fill appearances) are
  milestones 4–8. Exit unchanged: an Arabic EPUB paginates legibly; a
  shaped glyph run round-trips through `DocumentBuilder`. (XL,
  [design/shaping.md](design/shaping.md))
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
