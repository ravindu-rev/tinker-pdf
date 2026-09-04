# Roadmap

The goal, stated as a capability target: **every document a real producer
emits opens, renders provably the way the world renders it, round-trips,
signs, conforms — on every target, deterministically, with no unsafe code
and no silent failure.** The feature docs record what already holds; this
file lists what does not yet, and nothing else. What has left it is in git
history and in the design docs' own "as built" sections, not here.

Every item carries its **evidence** (a measured number or a named refusal),
its **exit criterion** (a test or CI job that runs — a roadmap item without
one is not done when the code lands, it is done when the check goes green),
and a **size band**: S ≈ 0.5 engine-months, M ≈ 1–2, L ≈ 2–4, XL ≈ 5–8.
L and XL items have a design doc in [design/](design/); where one is named
as owed, it is written before the item is scheduled. Scheduling within a
tier follows corpus hit-rate evidence (ruling 3, [rulings.md](rulings.md)),
not interest. Every number below was derived from the tree on 4 September
2026 by the command the commit message names; a number this file carries
and the tree contradicts is a defect in this file.

The measurements the tiers are ordered by, from `corpus/ratchet.json` and
its two font-bearing siblings:

| Measure | Value |
| --- | ---: |
| Corpus files (pdf.js 974, veraPDF 2 907, qpdf 637, PDF Association 7) | 4 525 |
| Render every page | 4 484 |
| Do not (pdf.js 11, veraPDF 1 timeout, qpdf 29) | 41 |
| Rendered with something reported, no faces / synthetic face / bundled faces | 929 / 299 / 330 |
| `rotate` held of asked | 4 030 of 4 211 |
| `crop` held of asked | 4 148 of 4 191 |
| `dpi` held of asked | 4 388 of 4 439 |

The suite stands at 4 403 passed, 0 failed, 43 ignored as
[verification.md](verification.md) records it, dated August 2026.

## What "best" means here

The target is not a list of features but a set of axes, each with a number
that can only move one way. A capability the field offers and this engine
lacks is a row in tier 5. An axis nobody measures is a row in tier 0, and it
comes first, because a claim with no ratchet behind it is the kind this
repository has already caught itself making. Nothing below names another
engine: a capability is described on its own terms, and "the field" means
what a reader of PDFs is entitled to expect.

| Axis | Measured today | What gives it a ratchet |
| --- | --- | --- |
| Correctness on documents nobody here wrote | 4 525 files from three readers' test suites and one association's examples — every one written to test a reader | tier 0: a corpus of documents real producers emitted for readers |
| Speed | six criterion operations, weekly, reporting and not gating; no committed number anywhere | tier 0: a baseline from a named machine, compared inside a band that machine's own swing set |
| Memory | 35 input-derived caps in `bounds_ledger.rs`; no peak measurement anywhere | tier 0: peak RSS per corpus file, recorded and ratcheted |
| Fidelity | arithmetic fixtures, metamorphic relations, committed fingerprints | tier 1's differential pairs and reviewed goldens; tier 0's decision on dated outside measurements |
| Capability coverage | tiers 2 to 5 of this file | each row's exit criterion |
| Footprint | 2.03 MB of wasm, 1.40 MB gzipped, gated at 2.5 MB in `release.yml` | already ratcheted |
| Surface | 123 C functions; four bindings, none projecting the whole facade; eight CLI subcommands, every one read-only | tier 3's bindings row; tier 5's CLI and bindings rows |
| Maturity | version 0.0.1, nothing published, one release run watched | tier 3's packages row |

## Tier 0 — measure what is not measured

These come before any new feature, with tier 1. Each row is an axis the
field judges an engine on and this repository has no number for, so a claim
about it today would be the kind of claim ruling 13 exists to prevent.

| Item | Evidence | Exit criterion | Size |
| --- | --- | --- | --- |
| **A corpus of production documents.** `corpus/corpora.lock` pins three readers' test suites and one association's seven examples; every file in the run was written to exercise a reader, and a reader that passes its own kind's tests has not met the world | the measurements table above; `corpus/corpora.lock` | a fourth class of entry in the lockfile — documents real producers emitted for readers, such as a sample of a public government-document set, fetched and pinned and never committed, with its licence in `corpus/README.md` like the others' — run nightly under its own ratchet row, and every failure attributed by producer | M |
| **Speed has no ratchet.** `bench.yml` runs six operations weekly and reports; `benches/engine.rs` commits no number; `corpus/report.json` records per-file `millis` and `corpus/ratchet.json` deliberately does not | `.github/workflows/bench.yml`, `crates/tinker-pdf/benches/engine.rs` | a baseline committed from a named machine, with that machine's run-to-run swing measured first so the band is a number rather than a guess; the weekly job compares against it with `--baseline` and fails outside the band; per-corpus wall time recorded beside the pass rate under the same discipline | M |
| **Memory has no measurement.** Thirty-five caps are properties of inputs and none of a process; the two largest runtime bounds, `MAX_PAGE_PIXELS` and `MAX_DECODED_STREAM`, are not ledger rows; the only peak-memory tests are `#[ignore]`d and need an outside watcher | `crates/tinker-pdf/tests/bounds_ledger.rs`, `epub_memory.rs`, `xps_memory.rs` | the corpus runner records each child's peak resident set in `report.json` and the per-corpus maximum in `ratchet.json` as a `<=` band; the two caps get ledger rows with clock-free firing tests | M |
| **Nothing renders pages in parallel.** `Document` is `Send + Sync`, proven by a four-thread test, and the library spawns no thread by policy, which is right for wasm; but no API, example or CLI flag uses it, so the property is unexercised | `crates/tinker-pdf/tests/tinker_parity.rs`, `docs/architecture.md` | `tpdf render --jobs N` and an example under `crates/tinker-pdf/examples/` drive a thread pool over pages; the facade stays thread-free and the decision is recorded | S |
| **No vectorisation anywhere.** No `std::arch`, no feature detection, no portable SIMD; every inner loop is scalar. Ruling 4 permits integer SIMD, since integer arithmetic is exact on every target | `grep -rn 'std::arch\|simd' crates` finds nothing | a measured speedup on the six benchmarks with every fingerprint unchanged on all four targets | M |
| **Fidelity against the world — a decision.** Ruling 13 bars any outside program from adjudicating a page, and rules that the committed output of a tool run once is a dated measurement rather than a check — the precedent the signature interop and the epubcheck record already set | [verification.md](verification.md), [design/signatures.md](design/signatures.md) | a recorded decision on whether a one-time dated visual measurement against outside viewers is admissible under that precedent. Until it is taken, tier 1's reviewed goldens are the only first-party answer | decision |

## Tier 1 — prove correctness

These come before any new feature, with tier 0. Ruling 13 says the engine agreeing with
itself is the only kind of proof this repository will have, which raises the
bar on what the checks must be: answers computable in closed form, bitstreams
transcribed from the standards' own annexes, published conformance data, and
thousands of documents nobody here authored. What this tier cannot contain is
stated once: the four properties that left with the oracles
([verification.md](verification.md), "What does not come back") do not return
and are not rows.

| Item | Evidence | Exit criterion | Size |
| --- | --- | --- | --- |
| Differential in-tree pairs — a Type 3 glyph against the same path, a tiling pattern against its unrolled content, a form XObject against its inlined operators, a shading pattern against `sh` | [design/render-verification.md](design/render-verification.md) milestone 5 has no test file behind it; `crates/tinker-pdf/tests/` holds `render_analytic.rs` and `epub_analytic.rs` and nothing differential | each pair byte-equal, each catching an injected divergence, counted | S |
| Reviewed goldens per operator family | milestone 6 of the same design, unstarted | each golden's header names reviewer, date and clause; a test reads the headers and fails on a missing field, in the `bounds_ledger.rs` style | S |
| The analytic tier's least-ink floor and its fingerprint enrolment | milestone 2 is "done, except" those two | both land in `render_analytic.rs` | S |
| `corpus/ratchet-fonts.json` is recorded under `synthetic-1`; the face `cargo xtask synth-face` writes has been `synthetic-2` since 31 August 2026, so the next `--fonts synthetic` run refuses to compare | `xtask/src/face.rs` says so in its own header; `grep synthetic corpus/ratchet-fonts.json` | a `--fonts synthetic --record` run, with the degraded count before and after in its commit message | S |
| Forty-one corpus files do not render every page, and none is attributed by class | the table above | each named in [verification.md](verification.md) with its reason — a refused capability, a timeout, a deliberately damaged fixture — or fixed; the ratchet moves | M |
| Metamorphic residue: 181 files fail `rotate`, 43 `crop`, 51 `dpi`. Only the anti-aliased-image-edge class is attributed | [design/image-edges.md](design/image-edges.md) attributes that one; the rest are counted and unexplained | the remainder attributed by class, in that document or a successor, or fixed. `dpi` on every file is a stated non-goal there | M |
| Four of the 39 fuzz targets have had no libFuzzer session: `pki_der`, `pki_cms`, `shape`, `signatures` | [verification.md](verification.md); `grep -c '^\[\[bin\]\]' fuzz/Cargo.toml` | a recorded session per target beside the others' execution counts | S |
| XPS: a part whose content type and magic bytes disagree draws the bytes and says nothing — a ruling 10 gap, and wiring JPEG XR widened it from two ordered pairs to twelve | [features/xps.md](features/xps.md) carries the row; `a_content_type_that_disagrees_with_the_bytes_draws_the_bytes_and_says_nothing` pins it; [design/jpeg-xr.md](design/jpeg-xr.md) names the fix — a leniency variant on `XpsElementDefect` pushed into `paint.rs`'s defects | the pinning test asserts the named leniency instead of the silence | S |
| `/MCR /Stm` and `/StmOwn` are not read, so two form XObjects on one page with overlapping marked-content ids would collide | six orphans across 717 tagged corpus files says it is not biting, which is not shown safe ([features/content-and-text.md](features/content-and-text.md)) | a `/Stm`-keyed lookup in `structure.rs`; a fixture with two forms sharing ids; the orphan count unchanged or lower | S |
| PDF/A: a defect that exists only in the bytes is invisible to a rule engine that reads the object graph, and the strict structural validator that walks those bytes is not joined to it | [features/pdfa.md](features/pdfa.md), [design/pdfa.md](design/pdfa.md) | `validate_pdfa` reports the strict tier's structural findings under their ISO 19005 clauses; the ledger rows of that class close | M |
| JBIG2 bounds: the three `bounds_ledger.rs` rows for symbol counts, symbol bytes and text instances are stated estimates, because no real OCR producer's JBIG2 is in the corpus to measure against | [design/jbig2-symbol-text.md](design/jbig2-symbol-text.md), milestone 7: "cannot be met as written" | measured against a real producer's output once one is in a corpus; until then the ledger rows say "estimate" | S, blocked on a fixture |
| JBIG2 6.5.8.2.2's reference offset is unpinned: two readings agree on every fixture in the tree | the same design doc | a fixture where the two readings differ, built with the test-only `MqEncoder` | S |
| JBIG2 tables B.14 and B.15 are reconstructed and only their one-bit code for zero is exercised, so a non-zero refinement delta is refused rather than decoded through the unverified part | [features/filters.md](features/filters.md) | a fixture exercising a non-zero delta; the guard lifts | S |
| `MAX_ICC_TAGS` and `MAX_ICC_BYTES` fire and have no `bounds_ledger.rs` rows; the corpus's largest profile is 718 672 bytes | [design/icc.md](design/icc.md) declines to claim them; `grep -i icc crates/tinker-pdf/tests/bounds_ledger.rs` finds none | two rows, each with a clock-free test that fires it | S |
| The LZMA decoder's matches and distances are exercised by one fixture, `7z-lzma2.cb7`, and the literal-only round trip shares one author | [design/comic-archives.md](design/comic-archives.md), Risks | a second real archiver's 7z with more than one folder, a BCJ chain, or a dictionary reset | S |
| JPEG XR decodes with nothing adjudicating it: the quantised lossy path past QP 1, the first-level overlap filter across a soft tile boundary, `HARD_TILING_FLAG`, `SHIFT_BITS`, `TRIM_FLEXBITS`, more than one QP per tile, and a damaged codestream's surviving values | [features/xps.md](features/xps.md) "Decoded but unadjudicated"; [design/jpeg-xr.md](design/jpeg-xr.md) | **none — this list cannot shrink by testing.** Under ruling 13 no second decoder may adjudicate, and T.832's conformance bitstreams are not freely licensed. It stays listed as unadjudicated, not as owed work | — |

## Tier 2 — close the named refusals, by measured reachability

Ordered by corpus count. The JBIG2 counts are the August 2026 census
(`crates/tinker-pdf/tests/jbig2_census.rs`, corpus-gated); `corpus/report.json`
in the tree carries no per-file records, so a codec gate with no census has no
count, and says so rather than guessing one.

| Item | Evidence | Exit criterion | Size |
| --- | --- | --- | --- |
| JBIG2 halftone regions and pattern dictionaries (T.88 6.6, 6.7; segment types 16, 20, 22, 23) — a third lineage after generic and symbol/text | 16 corpus files, 50 segments, and every one of the 16 carries halftone and nothing else — a sixth of the JBIG2 in the corpus. `Warning::Jbig2SegmentSkipped`; [design/jbig2-symbol-text.md](design/jbig2-symbol-text.md) names it as the roadmap item that work did not close | the 16 render with no placeholder; a picture coded halftone and coded generic decodes 0 pixels different, in the pattern of `jbig2_refinement.rs`, with fixtures from the test-only `MqEncoder`; the row leaves [features/filters.md](features/filters.md) | M. If it grows to L, `design/jbig2-halftone.md` first |
| JBIG2 custom Huffman tables (7.4.13, segment type 53) | 21 segments in 6 files; 5 of the 8 refining Huffman text regions in the corpus select one. `Warning::Jbig2VariantSkipped` | the 6 decode; an over-subscribed table still refuses with an asserted warning | S |
| JBIG2 transposed text regions (`TRANSPOSED` = 1) | 4 files; the flag is parsed and the region returns `None` | the 4 decode; a fixture coded both ways at 0 pixels different | S |
| JBIG2 retained bitmap-coding contexts (7.4.2) | 1 file: three segments consuming, two retaining — below ruling 3's line, named with its count | stays a named refusal with a reachability test until the count moves | S, unscheduled |
| The aggregate: 30 of the 103 JBIG2-bearing corpus files still report a JBIG2 warning, from 65 before the symbol lineage | [features/filters.md](features/filters.md) | the number, re-measured after each row above | — |
| ICC profiles this build cannot make a transform of: the `A2B*` tables of a v4 `mAB` profile (three tags in the corpus), a connection space other than XYZ, a data space with no transform here. All fall back to 8.6.5.5's alternate-space reading as `ColorSpace::Approximated`, and `CalRGB` and `CalGray` are approximated the same way | 143 of the corpus's 2 750 profiles, in 131 files ([features/rendering.md](features/rendering.md), [design/icc.md](design/icc.md)) | the `iccbased` count in the corpus report moves; what is still refused is refused by name with its count | M ([design/icc.md](design/icc.md)) |
| A transparency group declared in `/Lab` composites in RGB and is named; grey, RGB and CMYK groups composite in their own space | `RenderWarning::UnsupportedGroupSpace`; no corpus count is recorded | a Lab-group fixture composites in Lab, or the row keeps a count that says why not | S |
| JPEG arithmetic coding, and JPEG precision other than eight bits | `Capability::JpegArithmetic`, `Capability::Jpeg12Bit`; **no corpus count recorded** | a count first, from a corpus run that keeps per-file warnings; then a fixture from a real encoder | M each |
| JPX: the RGN, POC, PPM, PPT and CRG markers, the `BYPASS` and `TERMALL` code-block styles, component precision above 16 bits, tile-parts out of order | 3 of the 19 readable corpus JPX files refuse — one on a budget and two deliberately non-conformant veraPDF fixtures — so nothing here is reached by a real document | unscheduled under ruling 3; the rows stay named in [features/filters.md](features/filters.md) | — |
| `cmap` subtable format 13, and a Macintosh `cmap` in a non-Roman encoding | [features/fonts.md](features/fonts.md); no count recorded | a count first | S |

## Tier 3 — capabilities absent today

Ordered by leverage, not size.

| Item | Evidence | Exit criterion | Size |
| --- | --- | --- | --- |
| **Reading order of a right-to-left line in text extraction.** A searchable Arabic EPUB extracts backwards: `TextLine::rtl` is a report and `plain_text` follows content-stream order. Reversing a line by `rtl` is a decision about every PDF this engine reads, so it is pinned with its reasoning and not fixed | `crates/tinker-pdf/tests/epub_shaped.rs` pins the backwards line by name | a recorded decision, and the pin flipped to assert it | M |
| **The Universal Shaping Engine's remainder.** 32 of 333 Brahmic cases are not reproduced and nine of sixteen sections are not whole; two causes inside USE's cluster grammar account for 21 of the 32 — Kannada `SHKNDA-2/1` and Tai Tham `SHLANA-2/6` are one cluster shape wanting opposite orders, and no property this crate reads separates them. Named debts beside it: the Indic base-finding model, reph position, canonical ordering by combining class, Hangul decomposition, per-syllable GSUB confinement, `Default_Ignorable_Code_Point` | [design/shaping.md](design/shaping.md) milestone 5, "not met, and not relaxed"; `PASSING` and `TRIAGE` in `crates/tinker-pdf-shape/tests/text_rendering.rs`; [features/fonts.md](features/fonts.md) | sections whole, asserted by count; the priced alternative is a second cluster model | XL ([design/shaping.md](design/shaping.md)) |
| Shaping consumers still refused by name: a `CIDFontType0` with a bare CFF in a form field (`Shaper::new` takes an `&Sfnt`), a vertical CMap in a shaped fill, a simple `/DA` font; on the EPUB page, GPOS offsets below the `TextRun` level, and bidi whose unit is the run rather than the visual line, so a right-to-left line of two styled spans is two runs | [features/forms.md](features/forms.md), [features/fonts.md](features/fonts.md) | each refusal replaced by a fixture that renders joined or positioned | M |
| Scripts shaped but unverified — Syriac's Alaph, the topographical features of a joining Brahmic script, and the list by name in [features/fonts.md](features/fonts.md) | ruling 13 leaves a script with no first-party conformance fixture unadjudicated | a fixture per script, or the name stays on the list | — |
| **The PDF/A validator's 37 staged rules.** Agreement with the veraPDF corpus is 1 201 of 2 371 PDF/A files — 830 of 831 `pass`, 371 of 1 540 `fail`. The unagreed `fail` files fall into families: roughly 490 on the XMP predefined-schema rule, 250 in graphics clauses that run and not far enough, 90 in font clauses waiting on code-to-glyph mapping, 120 in annotation clauses that have no rule group at all; then Level A's structure rules, content-stream rules as an interpreter rather than a tokenizer, recursive validation of embedded files, the output intent's own ICC conformance, and 6.11 and 6.12 with fixtures and no rules | `PDFA_STAGED` in `crates/tinker-pdf/src/pdfa.rs`, counted; [features/pdfa.md](features/pdfa.md); [design/pdfa.md](design/pdfa.md) | the staged count moves down and the agreement ratchet up, one ledger class at a time | L ([design/pdfa.md](design/pdfa.md)) |
| PDF/A Level A is written by the builder and validated by nobody: the structure-tree rules are staged, so a level A file this build reports nothing about has had its tagging read by no one | [features/pdfa.md](features/pdfa.md) | a Level A rule group; `finish_archival`'s own output validated by it | M |
| **Tagged writing.** `/Alt`, `/ActualText`, `/E` and `/Lang` are not written on structure elements; there is no general tagging API beyond `PageBuilder::tagged`; the EPUB's tree carries no `/Alt` on images, no `/Lang`, no `<a>` as a `/Link` with its `/OBJR`, no table `/Headers`, `/Scope` or `/Summary`, and no `/RoleMap` | [features/content-and-text.md](features/content-and-text.md), [features/epub.md](features/epub.md), [design/tagged-pdf.md](design/tagged-pdf.md) | `epub_structure.rs` asserts each against the source XHTML; the PDF/UA census runs over this engine's own output | M |
| **Signatures.** ECDSA in a verdict (implemented, gated on 120 CAVP vectors, wired into no verdict because zero corpus signatures use it); RSASSA-PSS, named and not decoded; `adbe.pkcs7.sha1`, one corpus file and it is a fuzzer's output; a signature with no signed attributes; RFC 3161 timestamps, parsed and not validated; `GeneralNames`, carried and not decoded. And 6 of the 18 corpus signatures never parse — their `/ByteRange` does not bracket their `/Contents` — and are not attributed | [features/signatures.md](features/signatures.md); [design/signatures.md](design/signatures.md) | each wired against a published vector or a real signature; the six attributed as the file's defect or this engine's | S each; timestamps M |
| **Public-key encryption.** Triple DES as an envelope's content cipher is the one real gap: OpenSSL still emits `des-ede3-cbc` by default for older recipients. The 7.6.5 key derivation is held only to a second implementation by the same author, and that risk closes the day a real public-key-encrypted document arrives — no fetched corpus file uses the handler, and a grep for `/Adobe.PubSec` over every PDF under `corpus/files` finds none | [features/encryption.md](features/encryption.md); [design/pubsec.md](design/pubsec.md), Risks | a DES-EDE3 decrypt against a published vector; a real document in a corpus | S; the risk is not work |
| **Streaming open beyond page one.** The hint tables are read and checked by `validate::hints` and are not on the open path, so a page other than the first needs the main cross-reference table | [design/streaming-open.md](design/streaming-open.md) calls it a gap rather than a decision; 43 of the qpdf corpus's 45 linearized files render page one from their heads | page N of a linearized document opens inside a committed byte budget without the main table | M ([design/streaming-open.md](design/streaming-open.md)) |
| **Non-device colour spaces on write.** `DocumentBuilder` states `/DeviceGray`, `/DeviceRGB` and `/DeviceCMYK` and no ICC, CIE, `/Separation` or `/DeviceN` space; XPS's `{ColorConvertedBitmap}` refusal is waiting on exactly this | [features/creation.md](features/creation.md), [features/xps.md](features/xps.md) | `add_image` and the fill and stroke setters take an `/ICCBased` space; the XPS row closes with it | M |
| `ImageData::Compressed` cannot be constructed outside the workspace: `CompressedImage`, `ImageColorSpace`, `ImageFilter` and `SoftMask` are not re-exported by the facade | [features/creation.md](features/creation.md); `grep` on `crates/tinker-pdf/src/lib.rs` finds the names in a comment only | the four re-exported, with a doctest that builds one | S |
| **Editing.** A rotated or skewed text run is left uncut by redaction and reported; text inside a Type 3 glyph procedure or an annotation appearance stream is not rewritten; appearance synthesis covers seven annotation subtypes | [features/editing.md](features/editing.md); `a_rotated_run_is_left_alone_rather_than_cut_wrongly` | a rotated run cut along its own baseline; `/AP` and glyph-procedure streams rewritten; an `/AP` per further subtype | M; M; S per subtype |
| **Bindings.** Owed projections: the rest of `PageBuilder` (`encoded_text`, `glyphs`, `form`, `shading`, the two pattern setters, `set_ext_gstate`, `set_bleed_box`) and of `DocumentBuilder` (`add_named_font`, `add_cid_font`, `glyph_run`, `add_ext_gstate`, `add_form`, `add_shading`, `add_tiling_pattern`, `clear_image_resources`); `DocumentEditor`'s `import_page`, `keep_pages`, `flatten_annotations`, `add_annotation`, `reset_form`, `set_field_values`, `set_calculated_values`; `PageBuilder::tagged` as an `open_tag`/`close_tag` pair; a signature's `/Contents`, `/M`, `/ContactInfo`, `/Filter`, its warnings and `modifications`; signatures in Python and JavaScript; the read surface — outline, links, metadata, attachments, XMP, warnings; `authenticate_with_recipient` | [features/bindings.md](features/bindings.md); the C ABI exports 123 functions today | each through the C ABI and the three wrappers with a parity-script line; `cargo xtask bindings-parity` green | L ([design/bindings-write.md](design/bindings-write.md) covers the write half; the read half is owed a section there) |
| Published packages: `pip install`, `npm install` and `dotnet add package` do not work, by decision, until the facade freezes at 0.1.0 | [features/bindings.md](features/bindings.md); the release pipeline has run once and published nothing | a `workflow_dispatch` with `publish: true` | a decision, then S |
| PDF 2.0 deltas still "tracked": the UTF-8 string type; the 2.0 blend and dash clarifications | [pdf20-deltas.md](pdf20-deltas.md) | each row moves to a status with a doc behind it | S each |
| XML: three allowed public identifiers, no HTML named-entity table, and a `DOCTYPE` with an internal subset is refused | `crates/tinker-pdf-xml/src/lib.rs`; the exit criterion is already written in its tests | the entity table vendored from the published list, with provenance | S |

**Decision items, not commitments.** Each is a choice recorded so it is not an
omission, and each waits on evidence rather than on time: **OCR**, if ever, as
a host seam like `FontProvider` and not an engine; **container writing** — CBZ,
XPS and EPUB are read-only conversions; **an archival profile on
`WriteOptions`** — `rewrite` returns `Vec<u8>` with nowhere to put a refusal,
which is why the builder has one and the rewriter does not
([design/pdfa.md](design/pdfa.md)); **image encoders beyond deflate** — the
engine decodes JPEG, CCITT, JBIG2 and JPX and writes none, and partial image
redaction waits on this; **rendering intents** — measured and declined, since
no corpus file sets an ExtGState `/RenderingIntent` and five carry `ri`
([design/icc.md](design/icc.md)); **variation-aware, vertical, AAT and
Graphite shaping, and `JSTF`** — deferred under ruling 3
([design/shaping.md](design/shaping.md)); **forms** — the five catalog actions
(`WC`, `WS`, `DS`, `WP`, `DP`) that nothing runs, and a narrower production
than `NotADefinition` for provably inert document-scope statements, both wait
on a corpus count ([design/form-script-policy.md](design/form-script-policy.md)).

## Tier 4 — container depth

Debts each container format records about itself, in its feature doc's
refusal table.

| Item | Evidence | Exit criterion | Size |
| --- | --- | --- | --- |
| **EPUB: an `<img>` never becomes a box.** A fixed-layout comic from a real producer (KCC 11.0.1, `kcc-fixed-layout.epub`) reaches this build as correctly sized, correctly clipped, entirely blank pages, and the same is true of every `<img>` in a reflowable book — only an SVG `<image>` draws. Whether any warning names the dropped picture is to be checked first; if none does, that is a ruling 10 gap closed in the same change | `crates/tinker-pdf/tests/epub_fixed_layout.rs` pins the blank pages and says the test that paints a replaced element must come and delete it; [features/epub.md](features/epub.md) | the fixed-layout book's pages are more than one colour; a reflowable `<img>` is a replaced box with its intrinsic size; the pin deleted | M |
| **EPUB CSS: seventy properties known and unimplemented**, each reported as `UnimplementedProperty` with an element count, against 83 longhands implemented. The ones that change a paged output: `transform`, `opacity`, `overflow`, `box-shadow`, `text-shadow`, `border-radius`, the `background-*` image family, `list-style-*`, `counter-reset` and `counter-increment`, `quotes`, `text-transform`, `writing-mode`, `direction`, `unicode-bidi`, `hyphens`, `break-*`, `page`, `outline`, `clip-path`, `filter`, `mix-blend-mode`, `grid*`, `font-feature-settings`, `font-kerning`; and custom properties, which none of the committed books declares | `UNSUPPORTED_PROPERTIES` in `crates/tinker-pdf-css/src/property.rs`, counted; the census there measured 84 distinct names across the fetched corpus's 53 stylesheets and 42 across the committed 8 | scheduled by the fetched corpus's `UnimplementedProperty` counts, highest first; each landing deletes its name from the table | L — `design/epub-layout.md` is owed before the first L-sized slice |
| EPUB layout refusals, each a typed warning in `tinker-pdf-layout`: `column-span: all` laid out as `none`; `max-height` shorter than its content treated as `auto`; an atomic box taller than a page inside a table band, a flex line or a column, drawn where it starts; `inline-flex` as a block-level container; a block inside an inline laid out as a block; a table column's background and border never painted; anonymous row generation; `::first-line` and `::first-letter` parsed with no box; `content: url()`, `counter()`, `counters()` and the quote keywords generating nothing, and `quotes` itself; `:nth-child(An+B of S)` dropped; `@page` and `@supports` skipped | [features/epub.md](features/epub.md) | each row's warning stops firing on a reftest pair in `epub_reftest.rs` | S–M each |
| EPUB text set in a standard-14 fallback face loses characters past a simple font's 256 codes — 224 outside `WinAnsiEncoding` — counted as `UnrepresentedCharacters`. Whether an embedded face is exempt is to be verified: the comment predates `@font-face` | `crates/tinker-pdf/src/epub.rs` pushes the warning | a CID-keyed path for fallback text; the conservation harness unchanged | M |
| EPUB features tallied and not implemented: MathML layout (one fetched book has 71 `mathml` items), media overlays, `switch`, `remote-resources`; and a font provider attached after open cannot re-paginate, since advances decided the line breaks at open | [features/epub.md](features/epub.md) | MathML and overlays are decisions to take against corpus counts; re-pagination is a design question before it is work | decisions |
| EPUB corpus gaps: no committed stylesheet declares a cascade layer, so `@layer` is verified against this engine's own reading of css-cascade-5 and against no producer; no producer book carries a `.woff` or `.woff2` in its ZIP, so the `@font-face` path is held to containers this repository packs | [features/epub.md](features/epub.md); nine committed books | a real producer's book for each | S, fixture hunting |
| **SVG in the spine**, refused by name with a typed warning each: `<filter>`, `<mask>`, `<pattern>` as a paint, `<marker>` (32 in the fetched corpus, all on paths that also fill), `<foreignObject>`, SMIL, `<textPath>`, `<tref>` and `<altGlyph>`, `spreadMethod` `reflect` and `repeat` drawn as `pad`, a per-glyph `x`/`y`/`dx`/`dy`/`rotate` list past its first value, a clip path whose children are `<use>` or `<text>`, at-rules inside `<style>`; and group `opacity` flattened into each descendant's alpha, too dark where a fill and a stroke overlap | `crates/tinker-pdf-svg/src/lib.rs`; [design/svg.md](design/svg.md) | markers first, being the one with a count; group opacity as a real transparency group | S for markers, spread and glyph lists; M for group opacity and masks |
| **XPS.** `{ColorConvertedBitmap …}` refused, waiting on the write API above; a `ContextColor` in a gradient stop approximated; a profile with a channel count `/ICCBased` cannot state (`nCLR` past 1, 3 or 4) painted grey, for want of a tint transform from profile evaluation; `StyleSimulations` drawn unsimulated; per-stop alpha and a `ColorInterpolationMode` this build does not interpolate in, approximated; interleaved OPC packages refused, with no corpus package using one | [features/xps.md](features/xps.md) | each `XpsElementDefect` row leaves the doc with a conservation-census fixture | S for the bitmap, simulations and OPC; M for stops and alpha; L for `nCLR` |
| **Archives.** A JPEG 2000 entry in a CBZ is a placeholder page although the JPX decoder exists; ZIP method 14 is refused although the LZMA decoder exists; GIF and BMP need a decoder each; WebP and AVIF need a codec each; bzip2 and Zstandard; 7z's PPMd coder and its BCJ2 folder, which takes four input streams | [features/cbz.md](features/cbz.md); every CBZ fixture is in the tree, so there is no corpus count | wire the two decoders that exist first; the rest by evidence | S, S; M for GIF, BMP, bzip2, PPMd, BCJ2; L for WebP, AVIF, Zstandard |
| TIFF, reached from XPS and CBZ: `PhotometricInterpretation` 4, 5, 8 and 32803, `Compression` 6 and 34712, `SampleFormat` 2 and 3, `Predictor` 3, BigTIFF, and directories after the first | [features/filters.md](features/filters.md); no count recorded | by evidence | S–M each, unscheduled |
| JPEG XR's named refusals: fixed-point, half-float and float pixel formats; the packed sub-byte depths; CMYK, CMYKDIRECT, NCOMPONENT and RGBE; an unknown pixel-format GUID; the interleaved alpha plane; YUV420, YUV422 and YUVK; a windowed origin | [features/filters.md](features/filters.md), each reached by `jxr/tests/refusals.rs`; the platform encoder cannot emit most of them | a fixture from a real encoder per row | unscheduled |
| Fonts on write: a CID-keyed CFF under `add_cid_font` is refused; Symbol and ZapfDingbats have no bundled equivalent and are unreadable when nothing embeds them; WOFF2 table transforms this build does not read are refused by name | [features/fonts.md](features/fonts.md) | by evidence | S each |

## Tier 5 — capabilities the field expects

Every row here was checked absent or partial in source on 4 September 2026,
by grep against the facade, the two write surfaces, the CLI and the four
bindings, never against a doc alone. None has corpus evidence, because a
capability an engine lacks leaves no trace in a corpus run; the groups are
ordered by how much of the field uses them, which is a judgement and is
labelled as one. **Every L and XL row is owed a design doc before it is
scheduled, and none exists yet.** Where a row reverses a non-goal a feature
or design doc states, the reversal is a decision taken here and the doc
changes in the commit that schedules it.

### Output

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| PNG output from `Bitmap` and from `tpdf render` | `tpdf render` writes binary PNM; no PNG encoder; the deflate encoder exists (`filters/deflate.rs`, fixed Huffman and stored blocks) | `Bitmap::to_png`; `tpdf render` writes `.png`; every PngSuite file round-trips through this reader | S |
| Image encoders: JPEG; CCITT G4; JBIG2 generic region | decoders only, and the writer never re-encodes image bytes by contract | a baseline JPEG encoder held to this decoder and a published DCT vector set; a G4 encoder held to the T.4/T.6 coder the TIFF tests already carry; a JBIG2 encoder promoted from the test-only `MqEncoder` | M; S; M |
| Region and tile rendering on the facade | `RenderOptions` carries scale, format, cancel and annotations, and no clip; ruling 5's translated viewport is the mechanism and is already pinned byte-equal | `RenderOptions::region`; a tile byte-equal to the full-page subregion | S |
| CMYK page output; a premultiplied-alpha option; an anti-aliasing switch | `CmykA8` is internal to transparency groups; alpha is straight only; no quality knob | each a `RenderOptions` field with its own fingerprint; the switch changes no determinism claim | S each |
| A form XObject or an annotation rendered on its own | pages only | `Page::render_form`, `Page::render_annotation` | S |
| A retained page — a display list replayed at any scale | every render re-interprets the content stream; `tinker-pdf-svg`'s `Scene` is the shape | a recording `Device`; a replay byte-equal to a direct render at every scale the fingerprints use | M |
| PDF to SVG | SVG is input only | a `Device` that writes SVG 1.1, held by reading its output back through `tinker-pdf-svg` | M |
| PostScript and PCL output | none | decision: print pipelines are the only consumer | decision |

### Text

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| Word segmentation and word boxes | glyph, line and block quads; no word | `TextLine::words()` on UAX #29 boundaries, whose tables the shaper's UCD already carries | S |
| Structured text serialisation — JSON, XML, HTML — with fonts, sizes and boxes | the model exists and nothing serialises it; no serde in the tree and none wanted | hand-written writers, so zero third-party logic crates stays true; `tpdf text --json`; the font name carried on the span | M |
| Search options: case-sensitive, whole word, diacritic-insensitive; regular expressions | literal and case-insensitive, one argument | an options struct for the first three; regular expressions are a decision, since the tree has no regex engine and links none | S; decision |
| Inferred reading order for untagged pages — columns, running heads, footnotes — labelled as inferred | geometric line and block order; inference is a named refusal so a guess is never mistaken for the file's own order | an opt-in `ReadingOrder::Inferred` that is never the default, held to the 717 tagged corpus files by inferring with the tree hidden and scoring against it | L |
| Table reconstruction from geometry | none | opt-in, same discipline, held to the `/Table` elements the corpus carries | L |
| Hyphen rejoining at line ends | never, unless the producer wrote `/ActualText` | opt-in on `plain_text`: a soft hyphen always, a hard hyphen at a line end followed by a lower-case start; counted | S |
| Annotation contents and popups in extraction | field values only | `Page::annotations()` below exposes `/Contents`, `/T` and `/M` | S |

### Images and fonts

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| Image extraction with decoded samples and colour space | raw streams through `Document::cos()` only | `Page::images()` yielding decoded `ImageData` and its space; `tpdf images` | M |
| Font listing and extraction | the machinery exists one crate down and ruling 11 keeps it off the facade | `Document::fonts()` — name, type, embedded or not, subset tag, program bytes; `tpdf fonts` | S |
| A CJK fallback face | none bundled or fetched; the 202 predefined CMaps extract CJK text, and nothing draws it without a host face | an OFL face behind `bundled-fonts` — `deny.toml` already admits OFL-1.1 — kept out of the wasm default so the 2.5 MB gate holds | M |
| Hinting | outlines are unhinted by design | decision: an autohinter is L and a fidelity question ruling 13 cannot adjudicate; stem darkening is S and measurable as stem width at small pixel sizes; revisit with a corpus of small-text scans | decision |

### Document operations

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| Page labels; embedded files; an outline; `/Info` and XMP; a caller-supplied XMP packet; viewer preferences; trim and art boxes — each **on `DocumentEditor`** | page labels and attachments read only; outline and `/Info` on the builder only; XMP generated only for archival; viewer preferences, trim and art boxes neither read nor written | a typed setter for each, read back by this reader | S each |
| Named destinations on write | deliberately absent: a name is a destination only once the catalog carries a `/Names /Dests` tree | write the tree; ruling 6 still holds, and a named destination is never collapsed | S |
| Optional content: a facade reader, and writing groups and configurations | the reader is internal; nothing is written | `Document::layers()`; `DocumentBuilder::add_layer`; the editor toggles a default configuration | M |
| Watermark and stamp on existing pages | `append_content` is the primitive; nothing registers a resource on an existing page | `DocumentEditor::add_resource` and `stamp(page, form)` | M |
| Font subsetting on rewrite | build-side only; a rewrite copies every program untouched | glyph usage from the interpreter — the `ContentFilter` path redaction already walks — drives `cff_subset` and a TrueType subsetter on rewrite | M |
| Image recompression and downsampling on rewrite | never, by contract | an opt-in `WriteOptions::images` once the encoders above exist; original bytes untouched by default | M |
| Stream deduplication | declined until content hashing exists | SHA-256 over decoded bytes plus dictionary equality — identical means identical, since a wrong merge silently swaps two fonts | S |
| Sanitise: strip JavaScript, actions, embedded files, metadata | scripts are reported, nothing is stripped | `DocumentEditor::sanitise(Sanitise)` with a typed report of what left | S |
| Encryption on save below R6 — R4 with AES-128, RC4 — for readers that stop at 1.6 | R6 only | decision: the readers that need it are the whole reason | decision |

### Annotations and forms

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| A typed annotation model for every subtype | links and widgets only; no `Page::annotations()` | the model over 12.5.6's subtypes; every corpus annotation read and the refused ones counted by subtype | M |
| AcroForm field creation | signature fields only | `DocumentEditor::add_field` for text, check box, radio and choice, with an appearance | M |
| FDF and XFDF import and export | absent | both directions, held to the field tree this reader builds | S |
| ECMAScript for forms beyond the subset | a deliberate subset under `ScriptPolicy` | decision: a full engine is XL and [design/form-script-policy.md](design/form-script-policy.md) argues against running document code by default; grow the subset by the corpus count of refused constructs instead | decision |

### Signatures

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| A visible signature appearance | an invisible field only; the type says drawing one is a separate capability | an `/AP` from name, date, reason and an optional image, through the appearance synthesis annotations already use | S |
| Timestamps: creating one through a host seam, validating the token | tokens are located and handed out as opaque DER | a `Timestamper` seam like `Signer`, since the engine performs no I/O; validation held to a published token | M |
| Long-term validation: `/DSS` and `/VRI` | absent | written from host-supplied CRL and OCSP bytes | M |
| Public-key encryption on write | a non-goal because the engine would have to choose a certificate — and the caller can supply one | `Encryption::PublicKey { recipients }` sealing with caller-supplied certificates, held to the OpenSSL envelopes this reader already parses | M |

### Standards

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| PDF/UA validation | a measured abstention: 29 of 239 non-conforming fixtures caught, 210 abstained | a rule group that decides the decidable clauses and abstains by name on the rest | L |
| PDF/X validation and writing | a `GTS_PDFX` intent is tolerated and never checked | an ISO 15930 rule group; the archival profile grows a PDF/X flavour | L |
| PDF/E | absent | decision | decision |
| PDF 2.0: associated files, page-level output intents, namespaced structure types, the UTF-8 string type | encryption is the only 2.0 delta implemented | each read and written, and its row in [pdf20-deltas.md](pdf20-deltas.md) moved | S to M |

### Formats

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| A standalone SVG, a bare image, a loose HTML or XHTML file, each as a document | each refused as not-a-PDF, though the reader for each exists | `Document::open` sniffs and opens them | S each |
| HTML and CSS to PDF as a creation API | the cascade, the layout engine and the painter exist and are wired only inside the EPUB pipeline | `DocumentBuilder::from_html(markup, stylesheet, page box)`, held by the EPUB reftests | M |
| Markdown; FB2 | absent | a hand-written Markdown reader onto the HTML path; FB2 is XML onto the same | S; M |
| MOBI; DOCX | absent | decision: a DOCX layout engine is a word processor | decision |
| Writing EPUB, XPS and CBZ | a decision item already | unchanged | decision |

### Surface

| Item | Today | Exit criterion | Size |
| --- | --- | --- | --- |
| A user-facing CLI | eight subcommands, all read-only; the write half of the library is unreachable from it | merge, split, rotate, images, fonts, encrypt, decrypt, sign, attach, stamp, sanitise — each a wrapper over the facade with no logic of its own (ruling 11) | M |
| Java, Swift, Go and Ruby bindings | none | each over the C ABI in the .NET pattern, with the smoke and parity scripts; which first is a decision | M each |

## Named non-goals

Decisions, kept in one place so none is mistaken for an omission. Two kinds,
and the difference matters under the goal this file now states.

**Hard limits, for a reason outside this repository's power.**

- **XFA** — removed in ISO 32000-2 ([features/forms.md](features/forms.md)).
- **RAR's compression**, which has no published specification and one
  decoder whose licence bars derivation; and **RAR 4** entirely, which no
  producer here can write, so a decoder would be unadjudicated under ruling
  13 ([features/cbz.md](features/cbz.md),
  [design/comic-archives.md](design/comic-archives.md)).
- **A bundled sRGB profile** — the ICC's own carry no SPDX identifier, and
  which device an archival document's colours are for is the caller's
  statement ([design/pdfa.md](design/pdfa.md)).
- **Font directories** — `wasm32-unknown-unknown` has none, so `local()` is
  the host's to answer through `FontProvider`
  ([features/epub.md](features/epub.md)).
- **The four properties that left with the oracles** — a reader nobody here
  wrote accepting this engine's output, a second reading of an XPS package,
  a reference CSS implementation, an arbiter for an EPUB disagreement. Ruling
  13 retires them and [verification.md](verification.md) names them.

**Decisions this repository took and keeps, each revisitable by a row above
if the field's evidence asks.**

- **Shaping while reading a PDF** — `TJ` arrays are honoured as written
  ([features/fonts.md](features/fonts.md)).
- **Encryption inside archives** — ZipCrypto, AES in ZIP, 7z and RAR — and
  **multi-volume or sparse entries**
  ([design/comic-archives.md](design/comic-archives.md)).
- **EPUB scripting, `META-INF/signatures.xml`, real resource encryption**
  (this engine holds no key) ([features/epub.md](features/epub.md)).
- **Signing keys in the engine, revocation fetching, a bundled root store**
  ([design/signatures.md](design/signatures.md)); and **no callback into
  host code across the C ABI**, which is why `save_signed` and `Signer` are
  not projected ([design/bindings-write.md](design/bindings-write.md)).
- **Recipient shapes other than key transport** in public-key envelopes
  ([design/pubsec.md](design/pubsec.md)).
- **JPEG XR encoding**, and applying the container's orientation transform,
  which T.832 leaves to an application this crate is not (ruling 8)
  ([design/jpeg-xr.md](design/jpeg-xr.md)).
- **JPEG 2000 Part 2** (ISO/IEC 15444-2) — a marker Table A.2 does not
  define is refused as unknown rather than measured past
  ([features/filters.md](features/filters.md)).
- **SVG `<switch>` conditional processing** ([design/svg.md](design/svg.md)).

Six things the docs called non-goals are rows in tier 5 now, because the
field offers each and the reason given was a scope choice rather than a
limit: hinting, a visible signature appearance, timestamp creation, writing
a public-key-encrypted document, image encoders, and inferred reading order
for untagged pages. Each row says so.

## How this file changes

An item leaves this file when its exit criterion is green, and its feature
doc's refusal table loses the matching row in the same commit. An item
enters with evidence attached — a corpus number, a named refusal, an owed
assertion — or it does not enter. Design docs in [design/](design/) carry
scope, non-goals, design, milestones with concrete exit criteria, and
risks, in that order.
