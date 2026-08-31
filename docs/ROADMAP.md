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

Ordered by leverage, not size. **All eight have left it.** What follows
records what each one settled, because a row that simply disappeared would take
its evidence with it — and in this tier the evidence includes three diagnoses
that turned out to be wrong, which is the part worth keeping.

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

**PDF/A has left this list.** All six milestones of
[design/pdfa.md](design/pdfa.md) are green. `Document::validate_pdfa` runs
all four rule groups — metadata, syntax, fonts, colour — and returns
findings, where **a pass is an empty list and there is no boolean that
discards it**. `tpdf check --pdfa` exits by the verdict and prints which
groups ran beside it. `DocumentBuilder::finish_archival` writes a profiled
document, refusing forbidden features **at the call that makes them** so
nothing is discovered by the validator that the builder allowed. Level A is
claimable rather than staged, because tagged PDF landed first — the
concrete payoff of taking this list's own order rather than the evidence
order.

Four things worth carrying forward:

- **The denominator was wrong before the numerator was.** 2 896 corpus files
  carry a `-pass-`/`-fail-` annotation and 525 of them test a *different*
  standard — 434 PDF/UA, 85 TWG, 6 ISO 32000. A PDF/UA file annotated
  `pass` makes no PDF/A claim, and scoring it produced 195 spurious
  disagreements: a measurement measuring itself. Against the 2 371 that are
  PDF/A tests, agreement is **1 201**, from 1 017 before the font and colour
  groups.
- **One false positive, held at one on purpose.** 830 of 831 conforming
  files agree; the one that does not is a `%PDF-2.0` header this build reads
  literally against 6.1.2. An early font group scored 1 109 with 53 false
  positives and was thrown away — a validator that accuses conforming files
  is worse than one that stays quiet, because the accusations are what a
  caller stops believing first.
- **Writing the ledger's "I do not know why" rows is what made them
  findable.** All three turned out to be defects here rather than readings:
  `/Info` values were trimmed before comparison so `" veraPDF Consortium "`
  matched, a present-but-not-a-string `/Info` entry was skipped rather than
  reported, and the header rule accepted any digit so `%PDF-1.9` passed.
- **There is no vendored sRGB profile and the parameter is mandatory.** The
  ICC's own profiles carry a bespoke permission notice with no SPDX
  identifier, so they fail `cargo xtask vendor` at its first requirement
  rather than at its allowlist. A CC0 regeneration would clear that gate and
  was declined on a second argument: everything else vendored here is a
  published fact about a file format, and an ICC profile characterises a
  particular device — which device an archival document's colours are *for*
  is the caller's statement, not this engine's. Same shape as bundling no
  font faces.

**Not closed, and stated where a caller sees it**: the writer is checked by
the rule table that would also accept its mistakes. Near-miss twins narrow
that and do not close it. And `WriteOptions` has no archival profile,
because `rewrite` returns `Vec<u8>` with nowhere to put a refusal — asking
for one there would mean discovering violations in the validator, which is
the failure the builder's design exists to avoid.

**Streaming open has left this list.** `ByteSource` is the seam a host
implements to supply ranges; `Document::open_streaming` opens from one, and
the linearized fast path renders page one of a qpdf-corpus file with **zero
read ranges intersecting the tail past `/E`**. The byte budgets are
committed as `<=` ratchets: 13 753 bytes to open a 4.9 MB document (0.28%),
67 001 to open it and read one mid-file object, 29 696 to render page one of
a 1.6 MB linearized one.

Three things it settled that are worth keeping:

- **Arrival is not an input.** The determinism fingerprints run over a
  `ShreddedSource` that splits every read and must be bit-identical to a
  whole buffer. None moved.
- **A miss must never publish.** Counted injection found that `load` was
  publishing the null a `SourceMiss` produced — so a wasm host would fetch
  the range, ask again, and get the same null forever. Zero assertions
  caught the first injection, because every retry test re-opened the
  document and got a fresh cache; the guard that found it holds *one*
  document across a miss.
- **The design doc's milestone 3 was stale and was amended, not honoured.**
  It asked for a hint-table reader to be promoted from tests; a hardened
  production decoder already existed. It decodes 32 of the 45 linearized
  files in the qpdf corpus, 10 being password-sealed and 3 refused by name
  as qpdf's own deliberately malformed fixtures.

**The bindings write surface has left it too.** Ruling 11 in its plainest
form: the facade grew the closure-free equivalents first —
`DocumentEditor::checkpoint`/`restore` and
`DocumentBuilder::begin_page`/`push_page` — because closures do not cross
FFI, and the C ABI is then a mechanical wrapping of a Rust API that already
exists. `transaction()` became sugar over its own primitives.

Two scripts, fill-and-save and build-a-document, run from all four surfaces
and print byte-identical `WROTE sha256=` lines; `cargo xtask
bindings-parity` fails on a mismatch **or on a surface that ran and printed
nothing**, which is the failure that gets shipped. Every saved artefact
also passes the strict structural validator, which is what keeps four
byte-identical outputs from being identically wrong.

**Text shaping has left this list, and its diagnosis was wrong on the way
out.** All eight milestones of [design/shaping.md](design/shaping.md) are
green in the sense the design doc asks for — milestone 5's criterion is that a
partial section **names its reason**, and the reason on record was false.

**Adjudicated.** UAX #9 bidi over 770 241 `BidiTest` resolutions and 91 707
`BidiCharacterTest` cases. All 48 text-rendering-tests cases in the
CMAP/GSUB/GPOS sections, 29 more declined by name. Arabic joining and cursive
attachment 6/6 on SHARAN-1 — which is what settled cursive, since no section in
the original corpus contains a GPOS type 3 lookup at all. **301 of 333** Brahmic
cases, **seven of sixteen** sections whole.

**Demonstrated.** An Arabic EPUB paginates with joined forms and RTL line
order. An Arabic string through `DocumentBuilder::glyph_run` round-trips out of
text extraction. A filled Arabic form field renders joined — through
`/Identity-H`, any embedded CMap stream, any horizontal registry CMap, and a
non-identity `/CIDToGIDMap`. **GPOS offsets reach the page**: a mark sits at its
anchor rather than its advance.

Four things worth carrying forward, and three of them are about evidence rather
than about shaping:

- **The recorded diagnosis was false and arithmetic disproved it.** "The Indic
  shaper's base-finding" was blamed for SHKNDA-2 at 4/16 and SHKNDA-3 at 0/31.
  It accounted for **two** cases and **none**. Thirteen of NotoSansKannada's
  fourteen GDEF marks are *spacing* matras with real advances, the shaper zeroed
  every one, and the fixtures are exactly the cumulative `hmtx` sum. Conditioning
  mark widths on the plan bought 37 cases and two whole sections in one commit.
- **A fix was built, measured, and thrown away.** Making a matra adjacent to its
  base before the presentation features run gains 6 cases and loses **69**:
  `SHLANA-2/6` wants base/subjoined/mark where `SHKNDA-2/1` wants
  base/mark/subjoined for the same shape, and no property this crate reads
  separates them. Reverted, priced, and recorded — the alternative was a second
  cluster model.
- **Two guards were measured and found to hold nothing.** `MarkWidths` for
  non-Brahmic runs: setting it wrong fails *nothing*, because no face here has a
  GDEF mark with a non-zero advance. And the shaping fingerprint could not see
  the joiner, the reph, or a syllable-final base and halant — three behaviours it
  exists to pin. Both are written down beside the code rather than left as green
  runs.
- **A defect older than this work, surfaced and pinned rather than fixed.**
  A searchable Arabic EPUB extracts **backwards**: `TextLine::rtl` is a report,
  and `plain_text` follows content-stream order. Reversing a line by `rtl` is a
  decision about every PDF this engine reads, so it is pinned with its reasoning
  and left for a deliberate call.

**Still refused, by name**: CIDFontType0 with a bare CFF in a form field
(`Shaper::new` takes an `&Sfnt`); vertical CMaps, because drawing a run along x
that a viewer will stack is worse than a mark that announces itself; GPOS
offsets on the EPUB page below the `TextRun` level; and the Universal Shaping
Engine's remaining 32 cases, each classified `set`/`order`/`advance`/`offset` in
a committed triage table. Two causes account for 21 of them, both inside USE's
cluster grammar — the roadmap's "effectively unbounded" item, deliberately not
opened. Scripts with no conformance fixture stay listed by name in
[features/fonts.md](features/fonts.md) as shaped-but-unverified.

Decision items, not commitments: **OCR** (if ever, as a host seam like
`FontProvider`, not an in-engine engine) and **container writing** (CBZ,
XPS and EPUB are read-only conversions today). Named permanent non-goal:
**XFA** — removed in ISO 32000-2; stated so it is a decision rather than
an omission.

## Tier 4 — container depth

Debts each container format recorded about itself, in its feature doc's
refusal table.

- **EPUB** ([features/epub.md](features/epub.md)): WOFF/WOFF2, refused
  by name and with **no committed file behind the row** — no producer
  here emits one and OFL-1.1's reserved-name clause bars repacking a
  vendored face; the pinned float reading-order defect; multi-column;
  non-static `position`; `vertical-align`; SVG content documents;
  pseudo-elements (parsed, no box generated). `min`/`max` sizing is
  built, less one half of §10.7: a `max-height` shorter than its
  content is named (`MaxHeightAsAuto`) rather than obeyed.
- **XPS** ([features/xps.md](features/xps.md)): `VisualBrush`; JPEG XR
  **reaching the page** — the decoder is built and `xps/image.rs` still
  refuses the format at the element, which is the same shape TIFF was in
  before it was wired; remote resource-dictionary parts; `IsSideways` and
  odd `BidiLevel`.
- **CBZ** ([features/cbz.md](features/cbz.md)): CBR, CB7 and CBT, sized
  honestly — each means a hand-rolled decompressor, and **RAR 4 has no
  fixture this machine can produce**, so it closes as decoded-but-
  unadjudicated by name or not at all. A design doc is owed with the
  decoders.

## How this file changes

An item leaves this file when its exit criterion is green, and its feature
doc's refusal table loses the matching row in the same commit. An item
enters with evidence attached — a corpus number, a named refusal, an owed
assertion — or it does not enter. Design docs in [design/](design/) carry
scope, non-goals, design, milestones with concrete exit criteria, and
risks, in that order.
