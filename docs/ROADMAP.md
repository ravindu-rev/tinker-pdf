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

These come before any new feature. The suite is 2 948 tests proving the
engine agrees with itself, and ruling 13 says that is the only kind of proof
this repository will have. That raises the bar on what those tests must be
rather than lowering it: answers computable in closed form, bitstreams
transcribed from the standards' own annexes, published conformance data, and
thousands of documents nobody here authored.

- **Images are sampled differently at different resolutions.** The `dpi`
  relation — render at twice the scale, box-filter down, require agreement —
  fails on nine qpdf files and every one of them carries an image. The two
  PCLM files disagree on **23.4 %** of their pixels (107 592 of 459 869); the
  inline-image files on 4.4–5.4 %; the budget is 2 %. No text-only or
  vector-only file in 4 525 fails it. A wrong colour or a wrong shape commutes
  with the scale and leaves the relation green, so what this names is a *grid*
  mistake: an image's device rectangle, or its sampling of source pixels, is a
  function of the render scale when it must not be. Two of the nine were found
  only when the metamorphic budget was resited and they became eligible.
  Evidence: 9 of 582 files compared, August 2026, named per file in
  `corpus/report.json`. Exit: qpdf's `dpi` row holds on every image-carrying
  file, and a fixture pins one PCLM strip at two scales. (M)
- **A bundled face set, behind a feature.** Measured rather than argued:
  `corpus/ratchet-fonts.json` is the same 4 525 files with a face supplied,
  and **52 % of all reported degradation was the absence of one** — 1 045
  files down to 506, and in qpdf's corpus 530 down to 130
  ([features/fonts.md](features/fonts.md)). The base 14 are required to be
  available by 9.6.2.2, so a conforming file naming Helvetica and embedding
  nothing is one this engine cannot draw, which makes this a conformance gap
  rather than only a policy. Decided: a `bundled-fonts` feature carrying the
  Liberation family, **off by default**, because the `FontProvider` seam
  remains the right answer for a host with faces of its own. It brings back
  `deny.toml`'s OFL-1.1 entry, a `THIRDPARTY.md` section and a third corpus
  bar, all in the commit that adds the faces — which is what that allowlist
  comment already says it is waiting for. Exit: a `bundled-fonts` build
  renders base-14 text with no provider installed; a third ratchet row holds.
  (M)

## Tier 2 — close the named refusals, by measured reachability

Each of these is refused by name today (ruling 2 — the placeholder-plus-
warning contract), with corpus reachability measured.

- **JBIG2 symbol dictionary and text region.** The highest-reachability
  refusal in the engine: 103 files in the pdf.js corpus alone — more than
  JPX (19) or mesh shadings (10) had when they were built. It is what OCR
  pipelines emit, so most JBIG2 in circulation is this. The MQ coder is
  already shared and in its own module; the generic-region lineage is
  done. Exit: the symbol/text refusal rows leave
  [features/filters.md](features/filters.md); corpus hit-rate for the
  capability goes to ~zero. (L,
  [design/jbig2-symbol-text.md](design/jbig2-symbol-text.md))
- **Form XObject `/Resources`.** A form's own resource dictionary is
  consulted nowhere — tiling patterns' are, forms' are not (recorded in
  [features/rendering.md](features/rendering.md)); it is already the reason
  one of the 19 JPX corpus files never reaches the decoder. Exit: a form
  resolving its own resources renders; the JPX unreached file decodes. (M)
- **Transparency group colour spaces and page-level `/Group`.** The engine
  composites in RGB throughout and does not read a page-level `/Group`
  (11.4.7), so a group declared in CMYK or Lab blends in the wrong space.
  Exit: group colour space honoured in compositing, pinned by fixtures
  that differ only in the group's space. (M–L, folded into
  [design/icc.md](design/icc.md))
- **The JPX refusal list.** RGN, POC, PPM, PPT, CRG, five of Table A.19's
  six code-block styles, out-of-order tile-parts — every entry reachable
  and named, measured 4 refusals of 19 corpus files. Ruling 13 costs this
  item its cheapest source of fixtures: a codestream exercising a new
  partition can no longer be produced by asking an encoder for one, so each
  is hand-authored and transcribed, the discipline JBIG2 took from T.88's
  Annex H. Exit: refusal rows retire one by one as corpus files demand
  them. (M–L)
- **Full ICC colour.** ICC and CIE spaces are approximated by component
  count today, stated on the type. An own CMM — profile parsing,
  transforms, rendering intents — is the capability. Exit: ICC profiles
  drive conversion; known-answer tables computed from the specification's
  own equations hold for matrix/TRC profiles. (L,
  [design/icc.md](design/icc.md))
- **Incremental update with encryption.** An incremental save of an
  encrypted document needs the original file key plumbed to the
  incremental writer; today the combination is refused. Exit: fill a form
  in an encrypted file, save incrementally, and the saved file decrypts and
  passes the strict validator. (M)

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
