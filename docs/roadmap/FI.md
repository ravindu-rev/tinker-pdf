# Capability tier — formats in

[The roadmap](../ROADMAP.md) says how a step runs, what comes next, and which dependencies cross tiers. Each row below keeps its text as it was planned — Item, Today, Exit criterion, Doc, Depends on — and is cut into steps, each one workflow run.

Parity here means every input a widely deployed engine opens, and the
field's print-stream and archive formats besides. Each is ordered inside the
tier by corpus and producer evidence (ruling 3 as amended); none waits for
it.

## FI-01 — MOBI, PRC, PDB and AZW3/KF8

**MOBI, PRC, PDB and AZW3/KF8** (no DRM): PalmDOC, uncompressed and HUFF/CDIC records, the KF8 section, laid out by the XHTML path

**Today.** absent — the owner's parity decision of 9 October 2026; a decision row of the roadmap until 9 October 2026 ("MOBI; DOCX")

**Exit criterion.** books generated once by a producer (dated, bytes only, provenance recorded) open and draw as their XHTML twins pixel for pixel; HUFF/CDIC decoding held to the format's published description

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-02a — Office documents as text and structure

**Office documents as text and structure**: DOCX, XLSX, PPTX, ODF and HWPX translated to XHTML (paragraphs, headings, lists, tables, images, links) and laid out by the EPUB path

**Today.** absent — the owner's parity decision of 9 October 2026; the decision row this replaces declined it because "a DOCX layout engine is a word processor" — which is why the office rows are split into milestones (FI-02b…f)

**Exit criterion.** generated documents (built in the test from the ECMA-376 and ODF schemas) open with the expected text, structure and images

**Doc:** — · **Depends on:** FI-21

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-02b — DOCX layout, milestone 1

**DOCX layout, milestone 1**: paragraphs, runs, styles and their inheritance, numbering, sections and page setup

**Today.** absent — the owner's parity decision of 9 October 2026; a `.docx` is recognised only to be refused as no XPS (`crates/tinker-pdf/src/xps.rs:47`)

**Exit criterion.** analytic fixtures built from ECMA-376's rules (box positions computed in the test); determinism on four targets; visual review of producer documents is H-61

**Doc:** `design/office-layout.md` (owed) · **Depends on:** FI-02a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-02c — DOCX layout, milestone 2

**DOCX layout, milestone 2**: tables (grid, spans, borders, headers repeated), headers and footers, fields, footnotes and endnotes

**Today.** absent — the owner's parity decision of 9 October 2026; as FI-02b

**Exit criterion.** as FI-02b

**Doc:** `design/office-layout.md` (owed) · **Depends on:** FI-02b

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-02d — DOCX layout, milestone 3

**DOCX layout, milestone 3**: floating and anchored objects, text wrapping, DrawingML shapes, charts drawn from their cached data

**Today.** absent — the owner's parity decision of 9 October 2026; as FI-02b

**Exit criterion.** as FI-02b

**Doc:** `design/office-layout.md` (owed) · **Depends on:** FI-02c, FI-36

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-02e — Spreadsheet layout

**Spreadsheet layout** (XLSX, ODS): cells, number formats, merged cells, print areas, page breaks, charts from cached data

**Today.** absent — the owner's parity decision of 9 October 2026; no spreadsheet is read; `rg -i 'spreadsheetml' crates tools` finds only an associated file's media type in a test (`crates/tinker-pdf/tests/pdf20.rs:345`)

**Exit criterion.** as FI-02b, against ECMA-376 SpreadsheetML and ODF

**Doc:** `design/office-layout.md` (owed) · **Depends on:** FI-02a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-02f — Presentation layout

**Presentation layout** (PPTX, ODP): slides, layouts and masters, text boxes, shapes, images

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'pptx|presentationml|\bodp\b' crates tools` finds nothing

**Exit criterion.** as FI-02b, against PresentationML and ODF

**Doc:** `design/office-layout.md` (owed) · **Depends on:** FI-02a, FI-36

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-02g — RTF

**RTF** read and laid out

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\brtf\b' crates tools` finds nothing

**Exit criterion.** generated documents per control-word family

**Doc:** — · **Depends on:** FI-02a, FI-36

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-03 — Plain text documents

**Plain text documents** (`.txt`, `.log`) with encoding detection and box-drawing characters

**Today.** absent — the owner's parity decision of 9 October 2026; no opener takes plain text (`rg -i 'open_text|plain_text_document' crates` finds nothing; `plain_text` is extraction's output)

**Exit criterion.** fixtures in each detectable encoding open to their text, monospaced, one page per form feed

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-04 — Gzip-wrapped documents

**Gzip-wrapped documents** of any supported type

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'gzip|svgz' crates tools` finds nothing

**Exit criterion.** a gzipped copy of every committed fixture opens as the original

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-05 — Netpbm (PBM, PGM, PPM, PAM, PFM, and PKM, the 1-bit CMYK variant) and PSD composite images

**Netpbm (PBM, PGM, PPM, PAM, PFM, and PKM, the 1-bit CMYK variant) and PSD composite images** as documents and inside containers

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -iw 'netpbm|pgm|pam|psd' crates/tinker-pdf-filters/src` finds nothing (its `PPM` is JPEG 2000's packed-packet marker)

**Exit criterion.** Netpbm held to its format description with fixtures generated in the test; PSD's composite image held to fixtures generated once (dated, bytes only)

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-06 — Multi-image inputs

**Multi-image inputs**: multi-image Netpbm, BMP sub-images, stand-alone JBIG2 files, every frame of animated GIF and WebP as pages

**Today.** GIF and WebP open at their first frame (`features/filters.md:336-700`)

**Exit criterion.** each frame equals its frame decoded in the test; frame counts match the containers

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-07a — JPEG XL, milestone 1

**JPEG XL, milestone 1**: the container and the modular mode (lossless)

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'jxl|jpeg.?xl' crates tools` finds nothing

**Exit criterion.** the published conformance bitstreams for modular mode decode to their reference samples

**Doc:** `design/jpeg-xl.md` (owed) · **Depends on:** H-47

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-07b — JPEG XL, milestone 2

**JPEG XL, milestone 2**: VarDCT and lossless JPEG reconstruction

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'jxl|jpeg.?xl' crates tools` finds nothing

**Exit criterion.** the remaining conformance bitstreams; reconstructed JPEGs byte-equal to their originals

**Doc:** `design/jpeg-xl.md` (owed) · **Depends on:** FI-07a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-07c — JPEG XL as a PDF image filter, and HDR content

**JPEG XL as a PDF image filter, and HDR content**: the filter as its published PDF definition states it, and PQ and HLG transfer at high bit depth carried through rendering on the 16-bit pipeline and tone-mapped to the output space

**Today.** absent — the owner's parity decision of 9 October 2026; the filter's definition for PDF is not yet published (H-122)

**Exit criterion.** fixtures built from the published definition decode to FI-07a/b's decode of the same codestream as a bare file; the PQ and HLG transfers recomputed in the test from ITU-R BT.2100's equations

**Doc:** — · **Depends on:** FI-07b, RC-07, H-122

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-08 — AVIF

**AVIF** (AV1 intra decoding) in archives, as a bare image and in EPUB and HTML

**Today.** AVIF needs a codec: `PageDefect::UnsupportedFormat(Avif)` (`features/cbz.md:431`; `features/opening.md:337`), priced L by the archives row this replaces. Every CBZ fixture is in the tree, so there is no corpus count. Everything else that row named has left it — Zstandard as ZIP method 93 (a hand-rolled RFC 8878 decoder held to Appendix A and to the format project's golden files), 7z's BCJ2 and BCJ filters and PPMd coder, bzip2 as ZIP method 12 and 7z coder `040202`, ZIP method 14, JPEG 2000 pages, BMP and GIF, and WebP lossless and lossy (the lossy planes held to the WebM project's published VP8 test vectors, fetched pinned by commit and SHA-256 and run by CI's `vp8-vectors` job) — each recorded with its tests in [features/cbz.md](../features/cbz.md) and [features/filters.md](../features/filters.md); what those decoders still refuse by name is FI-25's

**Exit criterion.** the AV1 conformance streams for intra-only profiles decode to their reference outputs; re-priced in the design doc

**Doc:** `design/avif.md` (owed) · **Depends on:** H-47

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-09 — HEIC/HEIF

**HEIC/HEIF** (HEVC intra)

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'heic|heif|hevc' crates tools` finds nothing

**Exit criterion.** HEVC conformance streams for the still-picture profile

**Doc:** `design/heif.md` (owed) · **Depends on:** H-46 (patents)

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-10 — JPEG lossless and hierarchical processes

**JPEG lossless and hierarchical processes** (SOF3, SOF7, SOF11, SOF15; SOF5, SOF6, SOF13, SOF14)

**Today.** `JpegError::Lossless`, `JpegError::Differential` (`features/filters.md:802-803`); zero in the corpus; untracked

**Exit criterion.** decoded samples equal T.81 Annex H's predictors and Annex J's differential reconstruction recomputed in the test over streams built from the annexes' tables

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-11 — Image-container refusals

**Image-container refusals**: TIFF `Compression` 6 (old-style JPEG), SGI LogLuv and ThunderScan, `PhotometricInterpretation` 6 outside compression 7, CIELab at other depths and sample formats, `InkSet` other than 1; BMP `BI_JPEG`, `BI_PNG`, the `BI_CMYK` family and OS/2 2.x Huffman and RLE24

**Today.** each a typed `TiffError`/`BmpError` (`features/filters.md:807-817`). TIFF is reached from XPS and CBZ. `Compression` 6, which Technical Note 2 replaced: a file whose `JPEGInterchangeFormat` (tag 513) points at a complete interchange-format stream is readable by the existing JPEG decoder without a guess; one that carries only TIFF 6.0 §22's per-component tags is not; no count recorded. Compression 6 was once moved to the permanent refusals with masks and colour-filter arrays and was brought back on review (2 October 2026): a permanent refusal with no count recorded was a scheduling decision ruling 3 does not allow. `PhotometricInterpretation` 8 (CIELab) at 8 and 16 bits, CMYK (5), `SampleFormat` 2 and 3, `Predictor` 3, BigTIFF, `Compression` 34712 and directories after the first have left ([features/filters.md](../features/filters.md)); 4 (a mask) and 32803 (a CFA) are permanent (NG-10)

**Exit criterion.** per refusal, a fixture built from the TIFF 6.0 or BMP description (or generated once, dated) decodes to its known samples

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-12 — JPEG XR's named refusals

**JPEG XR's named refusals**: fixed-point, half-float and float formats; packed sub-byte depths; CMYK, CMYKDIRECT, NCOMPONENT and RGBE; unknown pixel-format GUIDs; the interleaved alpha plane; YUV420, YUV422 and YUVK; a windowed origin

**Today.** each a `JxrRefusal` reached by `crates/tinker-pdf-filters/src/jxr/tests/refusals.rs` ([features/filters.md](../features/filters.md)); the platform encoder cannot emit most of them, so a fixture per row needs a real encoder that can (H-106)

**Exit criterion.** a fixture from a real encoder per row (H-106) decodes within the lossless identity or the bound computed in the test

**Doc:** — · **Depends on:** H-106

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-13 — JPEG 2000 Part 2

**JPEG 2000 Part 2** markers and the extended file format features PDF can carry

**Today.** a marker Table A.2 does not define is refused as unknown rather than measured past (`features/filters.md:797`); a named non-goal of the roadmap until 9 October 2026

**Exit criterion.** each extension held to T.801's text and its annex examples where published

**Doc:** `design/jpx-part2.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-14 — Image orientation and resolution

**Image orientation and resolution**: EXIF and TIFF orientation applied to bare images and archive pages, JPEG XR's container orientation applied when converting, resolution reported

**Today.** orientation is not applied; JPEG XR's container orientation transform was a named non-goal of the roadmap until 9 October 2026, because T.832 leaves it to an application the leaf crate is not (ruling 8) — so it is applied in the facade when converting, never in the leaf ([design/jpeg-xr.md](../design/jpeg-xr.md))

**Exit criterion.** fixtures in all eight orientations draw upright; reported resolution equals the tags

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-15 — The Brotli filter

**The Brotli filter** (`/BrotliDecode`) in PDF streams

**Today.** a Brotli decoder exists in `tinker-pdf-filters` (`crates/tinker-pdf-filters/src/brotli.rs`) for WOFF2; no PDF filter name reaches it

**Exit criterion.** RFC 7932's test vectors through the PDF filter path; an unknown-filter fixture naming `/BrotliDecode` now decodes

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-16 — JBIG2 remainder

**JBIG2 remainder**: colour palette segments, the T.88 amendment features, random-access organisation

**Today.** non-goals of the design until 9 October 2026 (`design/jbig2-symbol-text.md:75-77, 100-102`)

**Exit criterion.** each held to T.88's text and the amendments' examples where published

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-17a — A PostScript interpreter, milestone 1 — the language

**A PostScript interpreter, milestone 1 — the language**: scanner, objects, virtual memory with save and restore, dictionaries and stacks, the operator set, error handling

**Today.** absent — the owner's parity decision of 9 October 2026; PostScript is read only as the type 4 calculator subset (`crates/tinker-pdf-color/src/function.rs`) and inside Type 1 font programs

**Exit criterion.** each operator held to the PostScript language reference's stated behaviour and examples, transcribed into tests; the interpreter fuzzed (K-04)

**Doc:** `design/postscript.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-17b — PostScript, milestone 2 — graphics and fonts

**PostScript, milestone 2 — graphics and fonts**: paths, painting, images, colour spaces, patterns and shading, halftones, `setpagedevice`, Type 1, 3, 42 and CID fonts

**Today.** absent — the owner's parity decision of 9 October 2026; as FI-17a

**Exit criterion.** analytic fixtures written in PostScript render to the same pixels as PDF twins this engine's builder makes from the same content

**Doc:** `design/postscript.md` (owed) · **Depends on:** FI-17a, FT-10

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-17c — PostScript, milestone 3

**PostScript, milestone 3**: DSC comments, EPS with preview headers, PostScript XObjects in PDF

**Today.** absent — the owner's parity decision of 9 October 2026; EPS and PostScript XObjects are named only by the PDF/A and PDF/X rules that refuse them

**Exit criterion.** EPS figures placed in documents draw as their stand-alone renders

**Doc:** `design/postscript.md` (owed) · **Depends on:** FI-17b

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-18a — PCL 5 (5e and 5c) input

**PCL 5 (5e and 5c) input**

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\bpcl\b|pcl5' crates tools` finds nothing

**Exit criterion.** analytic fixtures from the PCL 5 technical reference render to PDF twins this engine's builder makes from the same content

**Doc:** `design/pcl.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-18b — PCL XL input

**PCL XL input**

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'pclxl|pcl.xl|pcl6' crates tools` finds nothing

**Exit criterion.** as FI-18a, against the PCL XL protocol reference

**Doc:** `design/pcl.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-18c — HP-GL/2 input

**HP-GL/2 input**

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'hpgl|hp-gl' crates tools` finds nothing

**Exit criterion.** as FI-18a

**Doc:** `design/pcl.md` (owed) · **Depends on:** FI-18a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-19 — DjVu

**DjVu**: IW44 wavelets, JB2, BZZ, bundled and indirect documents, the text layer, annotations and outlines

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'djvu|iw44' crates tools` finds nothing

**Exit criterion.** JB2 and BZZ held exactly to the encoder's known input — bitmaps and text generated here, encoded once by an outside encoder (dated, bytes only) and committed beside their inputs; IW44 held to the specification's ZP-coder and wavelet arithmetic recomputed in the test, and an encoded photograph to a bound computed in the test from the encoder's input; the text layer, annotations and outlines to the values written

**Doc:** `design/djvu.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-20a — OFD

**OFD** (GB/T 33190): the package, pages, text, graphics and images

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\bofd\b' crates tools` finds nothing

**Exit criterion.** documents built in the test from GB/T 33190's schema, and documents a producer generated once (dated, bytes only) from content committed beside them, render to twins this engine's own builder makes from the same content

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-20b — SM2 and SM3

**SM2 and SM3** for OFD signatures

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\bsm2\b|\bsm3\b' crates tools` finds nothing

**Exit criterion.** the published GM/T test vectors

**Doc:** — · **Depends on:** FI-20a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-21 — Compound files and mail

**Compound files and mail**: CFB/OLE2 archives, EML and MSG messages with attachments, MHTML and WARC pages, CHM help files

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\bmhtml\b|\beml\b|\bwarc\b|\bchm\b|olecf' crates tools` finds nothing

**Exit criterion.** generated messages and archives open with every part; attachments carried as PDF attachments

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-22 — Customizable `<select>`'s `<selectedcontent>`

**Customizable `<select>`'s `<selectedcontent>`**

**Today.** Loose HTML that is not XML is read by `tinker_pdf_xml::html`, a hand-written WHATWG §13.2.5 tokenizer and §13.2.6 tree builder held to html5lib's suite vendored at `9329e64` (MIT, THIRDPARTY.md): **1 779 of the 1 784 tree-construction tests** that run with scripting disabled (`tests/html5lib.rs`, the five that do not pass named as a list), **7 028 of 7 028 tokenizer runs** (`src/html/suite.rs`; four lone-surrogate runs a Rust string cannot hold not attempted), and since 9 October 2026 **82 of 82 encoding tests**. **What is left** is the four tests of customizable `<select>`'s `<selectedcontent>` (`webkit02.dat` #45–#48): the element is built empty (`features/opening.md:325`), where the DOM makes a copy of the selected option when the parser pops an `<option>`, which needs the option selectedness algorithm; `tests1.dat` #77 fails on the shared 1 024-byte name cap, which is kept. Not attempted, by name: html5lib's `#script-on` tests (CD-12), its `scripted/` directory, and the standard's new processing instructions, which the vendored suite predates

**Exit criterion.** the selectedness algorithm and the copy; those four tests pass

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-23 — Multi-byte legacy encodings

**Multi-byte legacy encodings** — GBK, gb18030, Big5, EUC-JP, ISO-2022-JP, Shift_JIS, EUC-KR — in FB2, loose HTML and XHTML

**Today.** An FB2 in an 8-bit encoding opens since 3 October 2026: a declaration naming any of the WHATWG Encoding Standard's twenty-eight single-byte encodings, by any of its labels that XML 1.0's `EncName` [81] admits, is decoded by that encoding's index — vendored verbatim from `whatwg/encoding` into `tinker-pdf-xml/data/encoding-indexes` under the licence's BSD-3-Clause grant for portions incorporated into source code (THIRDPARTY.md says why that reading, H-90) — held by `tests/fb2.rs`'s `an_fb2_in_an_eight_bit_encoding_is_the_book_its_utf8_twin_is`. **What is left**: the multi-byte legacy encodings, which still open an FB2 as an empty page with `Markup(Truncated)` and make loose HTML report `MarkupDefect::EncodingNotDecoded` (`features/opening.md:327, 332`), each a state machine over an index of thousands of rows; and a measurement, since this repository has no FB2 corpus to say which encodings real books use (H-107)

**Exit criterion.** the multi-byte decoders from the same repository's indexes, each held to the vendored index and to the encoding standard's own test files; an FB2 in each encoding equals its UTF-8 twin

**Doc:** — · **Depends on:** H-90 (index licence)

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-24 — Sniffing refusals

**Sniffing refusals**: a bare BMP, an SVG or HTML root past byte 4 096

**Today.** `OpenError::NotAPdf` (`features/opening.md:335-336`)

**Exit criterion.** each opens; a PDF header in the first 4 096 bytes still wins

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-25 — Archive breadth

**Archive breadth**: 7z coders not read, ZIP shrink, implode, XZ and PPMd, Zstandard dictionaries, randomised bzip2, sparse tar entries

**Today.** refused by name (`features/cbz.md:414, 418, 427-429`) — among them a Zstandard frame that names a dictionary, and bzip2's randomised blocks, unwritten since 1999

**Exit criterion.** each method held to its published description with fixtures generated once (dated, bytes only)

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-26 — A picture past Annex C.2's 14 400 units

**A picture past Annex C.2's 14 400 units** kept inside the limit by `/UserUnit` or a scaled placement

**Today.** "not clamped, and owed" (`features/cbz.md:432`)

**Exit criterion.** an oversized picture opens as a page within 14 400 units and draws at a bounded cost

**Doc:** — · **Depends on:** ED-09

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-27 — A host resource provider

**A host resource provider** for what a loose file names beside itself and for an EPUB's remote resources (the engine still does no I/O)

**Today.** `StylesheetUnresolved`, `ImageNotDrawn { Unresolved }` and their kin (`features/opening.md:338`); `remote-resources` tallied as `UnimplementedFeature` ([features/epub.md](../features/epub.md))

**Exit criterion.** a test provider supplies the resources and the page draws as its self-contained twin

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-28 — Relayout of reflowable documents

**Relayout of reflowable documents**: re-paginate at a new page size, font size or margin, and after a font provider attached late, with locations (chapter, offset) that survive; layout accelerators saved and reloaded

**Today.** advances decide line breaks at open; "a font provider attached after open cannot re-paginate", since advances decided the line breaks at open (`ArchiveWarning::FontsAttachedAfterPagination`); a design question before it is work, the EPUB row this replaces said

**Exit criterion.** a book laid out at size A then B equals the book opened at B; a location resolves to the same text at both

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-29 — A reflowable view of fixed documents

**A reflowable view of fixed documents**: a PDF re-laid out from its structured text (and its tags where present) at any width

**Today.** absent — the owner's parity decision of 9 October 2026; re-layout exists only for EPUB's reflowable books (`crates/tinker-pdf/src/epub/package.rs:307-315`); no fixed page is re-laid out

**Exit criterion.** a tagged committed document's reflow extracts in the tree's order; an untagged one in the inferred order

**Doc:** — · **Depends on:** FT-30

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-30 — A custom document-handler seam

**A custom document-handler seam**: a host registers a format the engine does not read, synthesised through the builder

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'DocumentHandler|register_format' crates tools` finds nothing

**Exit criterion.** a test handler's format opens through `Document::open`

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-31 — Files over 4 GiB

**Files over 4 GiB**: 64-bit offsets in `ByteSource`, wide cross-reference fields, streaming paths that never hold the file

**Today.** the facade takes bytes; wasm32's address space bounds a slice

**Exit criterion.** a generated 5 GiB file opens and renders its last page within the MS-05 band (native targets)

**Doc:** — · **Depends on:** P-05

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-32 — Container streaming and prefetch hints

**Container streaming and prefetch hints**: EPUB and archives opened from a `ByteSource`; a linearized file's hint tables offered as prefetch ranges

**Today.** streaming open for PDF (`features/opening.md:218-249`); containers need all bytes

**Exit criterion.** a streamed book's first page draws after fetching only its parts; the prefetch list equals the hint tables' ranges

**Doc:** — · **Depends on:** MS-08

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-33 — XSL-FO input

**XSL-FO input**

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'xsl-fo|fo:block' crates tools` finds nothing

**Exit criterion.** analytic fixtures from the XSL 1.1 recommendation render to their HTML-path twins

**Doc:** `design/xsl-fo.md` (owed) · **Depends on:** CD-16

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-34 — A partly encrypted EPUB opens its unencrypted parts

**A partly encrypted EPUB opens its unencrypted parts** (ruling 2), each encrypted resource a typed warning

**Today.** the whole book is refused (`ArchiveRefusal::EncryptedResources`, `features/epub.md:604`)

**Exit criterion.** a generated book with one encrypted chapter opens with the rest drawn and one warning

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-35 — AFP and MO:DCA print streams

**AFP and MO:DCA print streams**

**Today.** absent — the owner's parity decision of 9 October 2026

**Exit criterion.** streams generated from the published MO:DCA reference render to PDF twins this engine's builder makes from the same content

**Doc:** `design/afp.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-36 — EMF, EMF+ and WMF read

**EMF, EMF+ and WMF read** — as pictures inside office and RTF documents, and as documents

**Today.** absent — the owner's parity decision of 9 October 2026; FO-19 writes EMF and nothing reads it (`rg -i 'metafile' crates` finds one comment in `bmp.rs` and no reader)

**Exit criterion.** fixtures built from the record definitions of the published MS-EMF, MS-EMFPLUS and MS-WMF specifications render to PDF twins this engine's builder makes from the same drawing; every record type each specification defines is drawn or named by a typed warning

**Doc:** `design/metafiles.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FI-37 — DXF drawings

**DXF drawings** as documents: entities, blocks, layers as optional content, line types, text and dimensions, and paper-space layouts as pages

**Today.** absent — the owner's parity decision of 9 October 2026; no CAD format is read

**Exit criterion.** fixtures built in the test from the published DXF reference's group codes render to PDF twins this engine's builder makes from the same geometry; each layer is an optional content group

**Doc:** `design/dxf.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*
