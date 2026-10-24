# Named non-goals

[The roadmap](../ROADMAP.md) holds the work; these are its hard limits.

**Hard limits only**, each for a reason outside this repository's power,
kept in one place so none is mistaken for an omission. Until 9 October 2026
this section also held decisions this repository took and kept; the owner's
parity decision moved every one of them into the tiers (the table at
the end of this section says where), because a scope choice is a row,
however late its tier. Each entry below names what would move it.

- **NG-01 — RAR's compression** (methods 1–5, solid archives), **RAR 4**
  entirely, and writing RAR. RAR's compression has no published
  specification and one decoder whose licence bars derivation; RAR 4 is
  written by no producer here, so a decoder would be unadjudicated under
  ruling 13 ([features/cbz.md](../features/cbz.md),
  [design/comic-archives.md](../design/comic-archives.md)). RAR 5's stored
  entries open; its encryption is SE-14.
- **NG-02 — The legacy Mac OS encoding tables**, and therefore a Macintosh
  `cmap` subtable read in a non-Roman encoding. The vendor's published
  mapping files disclaim warranty and grant no redistribution rights, so
  they have no SPDX identifier `deny.toml` allows and `cargo xtask vendor`
  refuses them — the same limit as NG-03, reached from the other direction.
  It costs nothing measurable: 28 of the corpus's 7 905 embedded faces carry
  such a subtable and **every one of them carries a Unicode subtable beside
  it**, which `glyph_for_char` prefers. What the engine owes and does is
  refuse rather than mis-map — above U+007F a Macintosh subtable returns
  nothing, so a caller can fall back instead of drawing the wrong glyph
  ([features/fonts.md](../features/fonts.md)). The licence wall is H-94's.
- **NG-03 — The ICC's own published profile files, bundled.** They carry
  no SPDX identifier, so `cargo xtask vendor` cannot admit them; which
  device an archival document's colours are for stays the caller's
  statement ([design/pdfa.md](../design/pdfa.md)). Narrowed on 9 October 2026
  by RC-19, which builds a first-party sRGB profile from IEC 61966-2-1's
  published numbers; the old non-goal, "a bundled sRGB profile", is that
  row.
- **NG-04 — The four properties that left with the oracles**: a reader
  nobody here wrote accepting this engine's output, a second reading of an
  XPS package, a reference CSS implementation, an arbiter for an EPUB
  disagreement. Ruling 13 retires them and [verification.md](../verification.md)
  names them. The dated outside comparison a person reads (H-61) is the
  admissible substitute, and it is never a check.
- **NG-05 — JPEG 2000 component precision above 16 bits.** T.800 Table A.11
  allows 1 to 38 — `Ssiz` runs `x000 0000` to `x010 0101`, "component
  sample bit depth = value + 1" — and this build refuses past 16. **The
  coefficient plane is what caps it, not the output sample.** E.1's
  dequantisation clamps a coefficient to `2^(R_b + 2)` sample units, and a
  plane entry is Q12 in an `i32`, so a coefficient on the clamp occupies
  `2^(R + 14)` of the plane's own format: `2^30` at 16 bits, which is
  `PLANE_BOUND` exactly, and `2^31` at 17, which an `i32` does not hold.
  That is not the whole reason and it would be revisitable on its own — an
  `i64` plane would hold it, at twice the 268 MB a 4096 x 4096
  four-component tile already costs, and at the price of re-proving the
  `MAX_PRODUCT` bound the fixed-point 9/7 rests on. **What makes it a limit
  is that there is nowhere to hand the result**: ISO 32000-1 Table 89 gives
  `/BitsPerComponent` as 1, 2, 4, 8 or 16 and nothing wider, so a widened
  sample would be narrowed again one stage later, where the narrowing is
  less visible rather than absent. It is **not the same bargain as JPEG's
  twelve-bit frames**, which decode and are narrowed to eight on the way out
  with `JpegPrecisionNarrowed` recorded: a JPEG coefficient path is fixed at
  the frame's own precision and only the *sample* needed narrowing, while a
  JPEG 2000 coefficient's magnitude grows with `R_b` by E.1, so there is no
  stage at which the wide value does not have to exist. T.800 says the same
  thing twice from its own side: Table A.11's footnote a) warns that "not
  all combinations of coding styles will allow the coding of 38-bit
  samples", and **every profile T.800 names caps the depth at or below
  this** — Table A.45's Profiles 0 and 1 at `7 <= Ssiz_i <= 11` (8-12 bits),
  Tables A.48, A.51 and A.52's Broadcast Contribution and IMF profiles at
  `7 <= Ssiz_i <= 15` (8-16 bits). Zero of the corpus's 39 JPX files reach
  it. `the_coefficient_plane_is_what_caps_precision` in
  `crates/tinker-pdf-filters/src/jpx/tests/bounds.rs` asserts the relation
  rather than the figure, so this entry fails a test if its premise stops
  being true ([features/filters.md](../features/filters.md)).
- **NG-06 — JPEG XR's unadjudicated list**: the quantised lossy path past
  QP 1, the first-level overlap filter across a soft tile boundary,
  `HARD_TILING_FLAG`, `SHIFT_BITS`, `TRIM_FLEXBITS`, more than one QP per
  tile, and a damaged codestream's surviving values. All of it **decodes**;
  nothing here checks that the result is *right*, and no amount of testing
  shrinks the list. Ruling 13 bars a second decoder from adjudicating an
  output, so the only thing that could close it is T.832's own conformance
  bitstreams, and those are not freely licensed. It can shrink if that
  changes (H-99) and by nothing this repository can do meanwhile.
  [features/xps.md](../features/xps.md) and
  [features/filters.md](../features/filters.md) carry the list;
  [design/jpeg-xr.md](../design/jpeg-xr.md) carries the reasoning and the
  three first-party evidence legs that stand in an oracle's place. **Not the
  same as JPEG XR encoding**, which was a scope choice and is now FO-20.
- **NG-07 — The arithmetic JPEG statistical model, adjudicated.** F.1.4.4's
  Tables F.4 and F.5, Table G.2, and B.2.4.3's DAC conditioning are read off
  the clause; the coder under them is pinned by T.81 Annex K.4.1, which
  adjudicates Annex D's *coder* outright and says nothing about the model
  that picks its contexts. **T.81 publishes no arithmetic-coded image** to
  adjudicate that with. *The search, so nobody repeats it* (20 September
  2026 unless noted): T.81 at `https://www.w3.org/Graphics/JPEG/itu-t81.pdf`
  (HTTP 200, 1 058 883 bytes, SHA-256 `631031d4…768bf0`), Annex K read
  section by section — K.1 and K.2 quantisation tables, K.3 Huffman tables
  and K.3.3's byte lists, K.4.1 the arithmetic coder vector and K.4's only
  subsection, K.5 to K.10 filters and guidance with no data — and a sweep
  for hexadecimal runs over the whole document finds only those, plus
  `X'FFFF0000'` in D.1; T.83 (ISO/IEC 10918-2), whose data ships on
  diskettes (H-98); T.84 (ISO/IEC 10918-3, HTTP 200, 419 607 bytes, SHA-256
  `cd714dd1…89f6173f`, 84 pages), whose clause 4.2.1 makes compliance data
  available "to parties who wish to determine compliance" and whose Annex G
  specifies the *structure* of the test streams and prints none. What holds
  it meanwhile: the decisions of each model are hand-derived from T.81's own
  figures, and the tests assert **which statistics bin every decision was
  taken against**, not only the coefficients, because a wrong bin decodes
  correctly until the bins have adapted; the search is written into
  `jpeg.rs`'s header. Zero corpus reachability: `jpeg_census.rs` walks 688
  files, 10 606 distinct `/DCTDecode` streams and 10 603 frames, every one
  SOF0, SOF1 or SOF2 at eight bits. What would move it: a real document
  using arithmetic coding, or published data (H-107). What would **not**: an
  arithmetic encoder written here, which would only prove the two halves of
  one reading agree (ruling 13).
- **NG-08 — Transport on `wasm32-unknown-unknown`**: network, file system,
  font directories, process launching. The target has none of them — no
  socket, no file, no process exists to call — and that is the platform's,
  not this repository's, to change; so `local()` there is the host's to
  answer through `FontProvider` ([features/epub.md](../features/epub.md)).
  Native transport is not a limit: every capability that needs it is a
  host-seam row that works on every target alike — FT-13, SG-10, SG-27,
  SG-28, FI-27, AN-20, FJ-12.
- **NG-09 — Shaping while reading a PDF.** `TJ` arrays are honoured as
  written ([features/fonts.md](../features/fonts.md)): the producer
  positioned every glyph, and re-shaping would draw a different document.
- **NG-10 — TIFF `PhotometricInterpretation` 4 and 32803.** A transparency
  mask "is used to define an irregularly shaped region of another image" and
  is not a picture; a colour-filter array is a raw sensor mosaic, and
  "turning it into a picture is demosaicing, which is an algorithm chosen,
  not a format read, and no data adjudicates one choice over another"
  ([features/filters.md](../features/filters.md)).
- **NG-11 — A page's state over time**: SMIL and CSS animation and
  transitions. A PDF page is static; the drawn state is time zero, which is
  what paper shows.
- **NG-12 — Proprietary DRM schemes** (vendor EPUB DRM, licence-server PDF
  handlers). Their keys and specifications are licensed by their owners; a
  host may plug a handler in through SE-08.
- **NG-13 — A machine verdict on PDF/UA's meaning clauses** (ISO 14289-1
  7.1–7.5, 7.7, 7.9; -2 8.2, 8.4). Undecidable by a program by
  construction — "an `/Alt` that lies is a well-formed `/Alt`"; the
  validator abstains by name and a person judges (H-62).
- **NG-14 — The other refusals the tree marks permanent**, which are answers
  rather than gaps: a run whose font is not in scope or whose text matrix is
  not finite cannot be measured for redaction (`features/editing.md:820-821`),
  because with no metrics no glyph can be placed and a position that is not
  a number cannot be compared with a rectangle; the redaction bounds
  `MAX_PLACEMENTS`, `MAX_FORM_COPY_BYTES` and 4 096 `Do`s a stream, each
  named; an XML internal subset and an undeclared entity in EPUB content
  (`features/epub.md:561-562`), because the internal subset is where every
  entity bomb lives, EPUB 3.3 §3.9 forbids one in a content document, and
  XML 1.0's *Entity Declared* constraint governs the other; and `local()`
  font sources (`features/epub.md:600`, NG-08).

**Brought into scope on 9 October 2026.** Before this, six things the docs
called non-goals had already become rows because the reason given was a
scope choice rather than a limit — hinting, a visible signature
appearance, timestamp creation, writing a public-key-encrypted document,
image encoders, and inferred reading order — and four of them closed. The
owner's parity decision moved the rest, each to the rows below; the docs
that stated them now point at those rows.

| Former non-goal or decision | Where it was stated | Row(s) now |
| --- | --- | --- |
| XFA ("removed in ISO 32000-2", filed as a hard limit) | the roadmap's hard limits; `features/forms.md:573` | AN-27…AN-34, AN-31 (FormCalc) — not a limit: the specification is published free and deployed forms carry it, and a reader is expected to read, lay out, script and flatten it |
| Signing keys in the engine; key parsing; signing arithmetic | the roadmap's named non-goals; `design/signatures.md:463-468`; `features/signatures.md:314`; `design/pubsec.md:62-66` | SG-01…SG-09, SG-23, SG-27, SG-28 |
| Revocation fetching | the roadmap's named non-goals; `design/signatures.md:469-481` | SG-10 (parse, evaluate, build requests; the host fetches — NG-08) |
| A bundled root store | the roadmap's named non-goals; `features/signatures.md:315` | SG-14 (trust lists from host bytes); bundling itself → H-27 |
| No callback into host code across the C ABI | the roadmap's named non-goals; `design/bindings-write.md:51-57` | SG-09, FJ-17, SU-21 |
| PAdES-LTA conformance levels | `design/signatures.md:487-490` | SG-07 |
| Curves other than P-256 and P-384; compressed points; Ed25519; SHA-3 | `features/signatures.md:318-319`; absent | SG-02, SG-03, SG-04 |
| Recipient shapes other than key transport; sealing to EC keys; AES-192 and RC2; per-group permissions; applying enveloped `/P` | the roadmap's named non-goals; `design/pubsec.md:45-93`; `features/encryption.md:219-226` | SE-07 |
| Encryption on save below R6 | a decision row of the roadmap | SE-01 |
| ECMAScript beyond the subset; catalog actions; automatic recalculation | a decision row of the roadmap; `design/form-script-policy.md:33-52`; `features/forms.md:569-571` | FJ-01…FJ-18 |
| OCR, "if ever, as a host seam" | the roadmap's decision items; `design/table-reconstruction.md:40-43` | OC-01…OC-17 |
| Hinting; stem darkening | a decision row of the roadmap; `features/fonts.md:926` | FT-04a–d |
| Rendering intents; overprint and spot simulation; N-channel output; black-point compensation; gamut mapping; iccMAX; named colour; proofing | the roadmap's decision items; `design/icc.md:34-76` | RC-01…RC-05, RC-20 |
| Variation-aware, vertical, AAT, Graphite shaping; `JSTF`; kashida | the roadmap's decision items; `design/shaping.md:66-83` | FT-07a–e |
| Search by regular expression | a decision row of the roadmap | FT-20 |
| PostScript and PCL output | a decision row of the roadmap | FO-08a/b, FO-09a–e |
| MOBI; DOCX | a decision row of the roadmap | FI-01, FI-02a–g |
| Writing EPUB, XPS, CBZ, tar, 7z | a decision row of the roadmap; `design/comic-archives.md:49-50` | FO-11, FO-12a/b/c |
| PDF/E | a decision row of the roadmap; `design/pdfa.md:64-66` | SD-13 |
| PDF/X-1:2001, X-2, X-5; the ICC registry lookup; conversion to PDF/X; trapping, screening, proofing | `design/pdfx.md:58-93`; `features/pdfx.md:142-145` | SD-11, SD-12, FO-24, RC-08, RC-05 |
| PDF/A conversion and repair; a general RDF store | `design/pdfa.md:67-77` | SD-02, SD-23 |
| Auto-tagging; repair for UA; assistive-technology output | `design/pdfua.md:48-77`; `design/tagged-pdf.md:48-68` | SD-07, SD-08, SD-09 (judging meaning stays a person's, H-62) |
| Full PDF 2.0 conformance; PDF 1.7 baseline and the four-format list as locked decisions | `architecture.md:26-35` | SD-18; the amendment → H-08 |
| Partial image redaction | `features/editing.md:822` | ED-36a |
| An archival profile on `WriteOptions` | the roadmap's decision items | SD-03 |
| Layout on the builder's own methods; shaping in `text`/`glyphs`; non-ASCII link targets | `features/creation.md:383-385` | ED-44, ED-45, ED-47 |
| Content editing beyond `append_content` | `design/bindings-write.md:47-50` | ED-01, ED-02 |
| Streaming write; async; prefetch; progressive rendering below a page; container streaming | `design/streaming-open.md:47-66` | ED-38, SU-14, FI-32, RC-15 |
| No threading API on the facade | `architecture.md:305-315` | P-15 (amends the locked decision; H-16) |
| Tables across pages; nested tables; CSV/JSON export | `design/table-reconstruction.md:44-64` | FT-32 |
| `object-fit`/`object-position`; `::first-line`; `max-height` on non-clipping boxes | `design/epub-layout.md:57-74`; `features/epub.md:580, 593` | CD-15, CD-02 |
| `@page` honoured over the caller's box | the EPUB layout row ("waits on the owner") | CD-02, CD-16; the ruling → H-13 |
| A CommonJS build | `features/bindings.md:1051` | SU-14 |
| Published packages before 0.1.0 | the packages row ("by decision") | SU-17; H-113, H-114 |
| `/Launch` and other actions never executed | `features/document-model.md:433` | AN-20 (typed host requests; the engine still launches nothing — NG-08) |
| JBIG2 symbol and text encoding; colour palettes; the amendments; random access | `design/jbig2-symbol-text.md:75-102` | ED-27, FI-16, OC-12 |
| JPEG XR encoding; applying the container's orientation | the roadmap's named non-goals | FO-20, FI-14 |
| JPEG 2000 Part 2 | the roadmap's named non-goals | FI-13 |
| SVG `<switch>`, `<filter>`, `<foreignObject>`, `<script>` | the roadmap's named non-goals; `design/svg.md:56-75, 107-110` | CD-09, CD-10, CD-11, CD-12 |
| EPUB scripting, `signatures.xml`, resource encryption | the roadmap's named non-goals | CD-12, SE-15 (vendor DRM stays NG-12) |
| Encryption inside archives; multi-volume and sparse entries | the roadmap's named non-goals | SE-14, FI-25 |
| MathML layout, media overlays, `switch`, `remote-resources` | the EPUB features row ("decisions") | CD-04, CD-05, CD-06, FI-27 |
| Vendor security handlers | `features/encryption.md:217` | SE-08 |
| Font directories, for native builds | the roadmap's hard limits | FT-13 (outside the engine; the engine limit stays NG-08) |
| A bundled sRGB profile | the roadmap's hard limits | RC-19 (first-party profile; the ICC's own files stay NG-03) |
| XPS signatures, print tickets, 3D, story fragments | `features/xps.md:167` | CD-13 |
| Bundled typefaces under the archival writer | `design/pdfa.md:78-83` | FT-01, FT-02, FT-12 |
